//! HTTP request handlers.

use super::types::{
    AccountInfo, AccountsResponse, AdoptAccountRequest, BotConfigResponse, BotInfo,
    DeleteUsernameRequest, HealthResponse, ProfileResponse, RegisterRequest, RegisterResponse,
    SetUsernameRequest, StatusResponse, UnregisterRequest, UpdateBotConfigRequest,
    UpdateProfileRequest, UsernameResponse, VerifyRequest, VerifyResponse,
};
use super::AppState;
use crate::error::ProxyError;
use crate::registry::{normalize_phone_number, PhoneNumberRecord, RegistrationStatus};
use axum::{
    extract::{Path, State},
    Json,
};
use tracing::{info, warn};

/// Health check endpoint.
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let registry = state.registry.read().await;
    let signal_healthy = state.signal_client.health_check().await;

    Json(HealthResponse {
        status: "ok".to_string(),
        registry_count: registry.count(),
        signal_api_healthy: signal_healthy,
    })
}

/// Initiate registration for a phone number.
pub async fn register_number(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ProxyError> {
    // Normalize phone number
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Registration request received");

    // Check if already registered
    let registry = state.registry.read().await;
    if let Some(record) = registry.get(&number) {
        match record.status {
            RegistrationStatus::Verified => {
                warn!(phone_number = %number, "Attempted re-registration of verified number");
                return Err(ProxyError::AlreadyRegistered(number));
            }
            RegistrationStatus::Pending => {
                // Allow retry if ownership matches
                if !record.verify_ownership(request.ownership_secret.as_deref()) {
                    return Err(ProxyError::OwnershipProofMismatch);
                }
                // Fall through to retry registration
            }
            RegistrationStatus::Failed => {
                // Allow re-registration for failed attempts
            }
        }
    }
    drop(registry);

    // Proxy to Signal CLI REST API
    // If Signal says "already registered", try to unregister first and retry
    let register_result = state
        .signal_client
        .register(&number, request.captcha.as_deref(), request.use_voice)
        .await;

    if let Err(ProxyError::SignalApi(ref msg)) = register_result {
        if msg.contains("already registered") || msg.contains("AlreadyVerified") {
            info!(phone_number = %number, "Account already registered, attempting to unregister and retry");
            // Try to unregister the stale registration
            if state.signal_client.unregister(&number).await.is_ok() {
                // Retry registration
                state
                    .signal_client
                    .register(&number, request.captcha.as_deref(), request.use_voice)
                    .await?;
            } else {
                // Unregister failed, return original error
                return Err(register_result.unwrap_err());
            }
        } else {
            return Err(register_result.unwrap_err());
        }
    } else {
        register_result?;
    }

    // Record the registration attempt
    let record = PhoneNumberRecord::new_pending(
        number.clone(),
        request.ownership_secret.as_deref(),
        request.model.clone(),
        request.system_prompt.clone(),
    );

    let mut registry = state.registry.write().await;
    registry.insert(number.clone(), record);

    // Persist to encrypted storage
    state.store.save(&registry).await?;

    info!(phone_number = %number, "Registration initiated, awaiting verification");

    Ok(Json(RegisterResponse {
        phone_number: number,
        status: "pending".to_string(),
        message: "Verification code sent. Use /v1/register/{number}/verify/{code} to complete."
            .to_string(),
    }))
}

/// Verify registration with code.
pub async fn verify_registration(
    State(state): State<AppState>,
    Path((number, code)): Path<(String, String)>,
    Json(request): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ProxyError> {
    // Normalize phone number
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Verification request received");

    // Check registration exists and is pending
    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    if record.status != RegistrationStatus::Pending {
        return Err(ProxyError::NotFound(number));
    }

    // Verify ownership
    if !record.verify_ownership(request.ownership_secret.as_deref()) {
        return Err(ProxyError::OwnershipProofMismatch);
    }
    drop(registry);

    // Submit verification code to Signal CLI
    state
        .signal_client
        .verify(&number, &code, request.pin.as_deref())
        .await?;

    // Mark as verified
    let mut registry = state.registry.write().await;
    if let Some(record) = registry.get_mut(&number) {
        record.mark_verified();
    }

    // Persist
    state.store.save(&registry).await?;

    info!(phone_number = %number, "Registration verified successfully");

    Ok(Json(VerifyResponse {
        phone_number: number,
        status: "verified".to_string(),
        message: "Phone number registered successfully.".to_string(),
    }))
}

/// Get registration status for a phone number.
pub async fn get_status(
    State(state): State<AppState>,
    Path(number): Path<String>,
) -> Result<Json<StatusResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;

    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    Ok(Json(StatusResponse {
        phone_number: record.phone_number.clone(),
        status: record.status.clone(),
        registered_at: Some(record.registered_at.to_rfc3339()),
    }))
}

/// List all registered accounts.
pub async fn list_accounts(State(state): State<AppState>) -> Json<AccountsResponse> {
    let registry = state.registry.read().await;
    let accounts: Vec<AccountInfo> = registry
        .list_all()
        .into_iter()
        .map(|r| AccountInfo {
            phone_number: r.phone_number.clone(),
            status: r.status.clone(),
            registered_at: r.registered_at.to_rfc3339(),
            model: r.model.clone(),
            system_prompt: r.system_prompt.clone(),
            username: r.username.clone(),
        })
        .collect();

    let total = accounts.len();
    Json(AccountsResponse { accounts, total })
}

/// Unregister a phone number.
pub async fn unregister(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<UnregisterRequest>,
) -> Result<Json<VerifyResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Unregister request received");

    // Check registration exists
    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    // Verify ownership
    if !record.verify_ownership(request.ownership_secret.as_deref()) {
        return Err(ProxyError::OwnershipProofMismatch);
    }
    drop(registry);

    // Unregister from Signal CLI
    state.signal_client.unregister(&number).await?;

    // Remove from registry
    let mut registry = state.registry.write().await;
    registry.remove(&number);

    // Persist
    state.store.save(&registry).await?;

    info!(phone_number = %number, "Phone number unregistered");

    Ok(Json(VerifyResponse {
        phone_number: number,
        status: "unregistered".to_string(),
        message: "Phone number unregistered successfully.".to_string(),
    }))
}

/// Debug endpoint: List accounts registered in Signal CLI (not our registry).
pub async fn debug_signal_accounts(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ProxyError> {
    let accounts = state.signal_client.list_accounts().await?;
    Ok(Json(serde_json::json!({
        "signal_cli_accounts": accounts,
        "note": "These are accounts Signal CLI knows about, not our proxy registry"
    })))
}

/// Debug endpoint: Force unregister a number from Signal CLI (bypasses registry check).
pub async fn debug_force_unregister(
    State(state): State<AppState>,
    Path(number): Path<String>,
) -> Result<Json<serde_json::Value>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    warn!(phone_number = %number, "Force unregister requested (debug endpoint)");

    state.signal_client.unregister(&number).await?;

    // Also remove from our registry if present
    let mut registry = state.registry.write().await;
    registry.remove(&number);
    state.store.save(&registry).await?;

    Ok(Json(serde_json::json!({
        "status": "unregistered",
        "phone_number": number,
        "message": "Force unregistered from Signal CLI"
    })))
}

/// Update profile for a registered phone number.
pub async fn update_profile(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<UpdateProfileRequest>,
) -> Result<Json<ProfileResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Profile update request received");

    // Check registration exists and is verified
    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    if record.status != RegistrationStatus::Verified {
        return Err(ProxyError::NotFound(number));
    }

    // Verify ownership
    if !record.verify_ownership(request.ownership_secret.as_deref()) {
        return Err(ProxyError::OwnershipProofMismatch);
    }
    drop(registry);

    // Update profile via Signal CLI
    state
        .signal_client
        .update_profile(&number, request.name.as_deref(), request.about.as_deref())
        .await?;

    info!(phone_number = %number, "Profile updated successfully");

    Ok(Json(ProfileResponse {
        phone_number: number,
        message: "Profile updated successfully.".to_string(),
    }))
}

/// Set username for a registered phone number.
pub async fn set_username(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<SetUsernameRequest>,
) -> Result<Json<UsernameResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, username = %request.username, "Set username request received");

    // Check registration exists and is verified
    {
        let registry = state.registry.read().await;
        let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

        if record.status != RegistrationStatus::Verified {
            return Err(ProxyError::NotFound(number));
        }

        // Verify ownership
        if !record.verify_ownership(request.ownership_secret.as_deref()) {
            return Err(ProxyError::OwnershipProofMismatch);
        }
    }

    // Set username via Signal CLI
    let info = state
        .signal_client
        .set_username(&number, &request.username)
        .await?;

    // Store username in our registry
    {
        let mut registry = state.registry.write().await;
        if let Some(record) = registry.get_mut(&number) {
            record.set_username(info.username.clone());
        }
        state.store.save(&registry).await?;
    }

    info!(phone_number = %number, username = ?info.username, "Username set successfully");

    Ok(Json(UsernameResponse {
        phone_number: number,
        username: info.username,
        username_link: info.username_link,
        message: "Username set successfully.".to_string(),
    }))
}

/// Delete username for a registered phone number.
pub async fn delete_username(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<DeleteUsernameRequest>,
) -> Result<Json<UsernameResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Delete username request received");

    // Check registration exists and is verified
    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    if record.status != RegistrationStatus::Verified {
        return Err(ProxyError::NotFound(number));
    }

    // Verify ownership
    if !record.verify_ownership(request.ownership_secret.as_deref()) {
        return Err(ProxyError::OwnershipProofMismatch);
    }
    drop(registry);

    // Delete username via Signal CLI
    state.signal_client.delete_username(&number).await?;

    info!(phone_number = %number, "Username deleted successfully");

    Ok(Json(UsernameResponse {
        phone_number: number,
        username: None,
        username_link: None,
        message: "Username deleted successfully.".to_string(),
    }))
}

/// Adopt an existing Signal CLI account into the proxy registry.
/// This is for accounts that were registered directly with Signal CLI before the proxy was set up.
pub async fn adopt_account(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<AdoptAccountRequest>,
) -> Result<Json<VerifyResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Adopt account request received");

    // Check if already in our registry as verified
    let registry = state.registry.read().await;
    if let Some(record) = registry.get(&number) {
        if record.status == RegistrationStatus::Verified {
            return Err(ProxyError::AlreadyRegistered(number));
        }
    }
    drop(registry);

    // Verify the account exists in Signal CLI
    let signal_accounts = state.signal_client.list_accounts().await?;
    if !signal_accounts.contains(&number) {
        return Err(ProxyError::SignalApi(format!(
            "Account {} not found in Signal CLI. Register it first.",
            number
        )));
    }

    // Create a verified record in our registry
    let mut record = PhoneNumberRecord::new_pending(
        number.clone(),
        request.ownership_secret.as_deref(),
        request.model.clone(),
        request.system_prompt.clone(),
    );
    record.mark_verified();

    let mut registry = state.registry.write().await;
    registry.insert(number.clone(), record);

    // Persist to encrypted storage
    state.store.save(&registry).await?;

    info!(phone_number = %number, "Account adopted into registry");

    Ok(Json(VerifyResponse {
        phone_number: number,
        status: "verified".to_string(),
        message: "Account adopted into registry successfully.".to_string(),
    }))
}

/// Update bot configuration (model, system prompt).
pub async fn update_bot_config(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Json(request): Json<UpdateBotConfigRequest>,
) -> Result<Json<BotConfigResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;
    info!(phone_number = %number, "Bot config update request received");

    // Check registration exists and is verified
    {
        let registry = state.registry.read().await;
        let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

        if record.status != RegistrationStatus::Verified {
            return Err(ProxyError::NotFound(number));
        }

        // Verify ownership
        if !record.verify_ownership(request.ownership_secret.as_deref()) {
            return Err(ProxyError::OwnershipProofMismatch);
        }
    }

    // Update config in registry
    let (model, system_prompt) = {
        let mut registry = state.registry.write().await;
        let record = registry.get_mut(&number).ok_or(ProxyError::NotFound(number.clone()))?;
        record.update_config(request.model.clone(), request.system_prompt.clone());
        let result = (record.model.clone(), record.system_prompt.clone());
        state.store.save(&registry).await?;
        result
    };

    info!(phone_number = %number, "Bot config updated successfully");

    Ok(Json(BotConfigResponse {
        phone_number: number,
        model,
        system_prompt,
        message: "Bot configuration updated successfully.".to_string(),
    }))
}

/// Get bot configuration.
pub async fn get_bot_config(
    State(state): State<AppState>,
    Path(number): Path<String>,
) -> Result<Json<BotConfigResponse>, ProxyError> {
    let number = normalize_phone_number(&number).map_err(ProxyError::InvalidPhoneNumber)?;

    let registry = state.registry.read().await;
    let record = registry.get(&number).ok_or(ProxyError::NotFound(number.clone()))?;

    if record.status != RegistrationStatus::Verified {
        return Err(ProxyError::NotFound(number));
    }

    Ok(Json(BotConfigResponse {
        phone_number: record.phone_number.clone(),
        model: record.model.clone(),
        system_prompt: record.system_prompt.clone(),
        message: "Bot configuration retrieved.".to_string(),
    }))
}

/// List all verified bots (public endpoint).
pub async fn list_bots(State(state): State<AppState>) -> Json<Vec<BotInfo>> {
    // Collect verified records and release the lock before async calls
    let verified_records: Vec<_> = {
        let registry = state.registry.read().await;
        registry
            .list_all()
            .into_iter()
            .filter(|r| r.status == RegistrationStatus::Verified)
            .cloned()
            .collect()
    };

    let mut bots = Vec::new();
    for r in verified_records {
        // Generate Signal.me link
        let phone_digits = r.phone_number.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
        let signal_link = format!("https://signal.me/#p/+{}", phone_digits);

        // Use username as display name, or phone number as fallback
        let username = r.username.clone().unwrap_or_else(|| r.phone_number.clone());

        // Extract description from first line of system prompt
        let description = r.system_prompt.as_ref().and_then(|p| {
            p.lines().next().map(|line| {
                if line.len() > 100 {
                    format!("{}...", &line[..100])
                } else {
                    line.to_string()
                }
            })
        });

        // Fetch identity key (safety number) from Signal CLI
        let identity_key = match state.signal_client.get_identity(&r.phone_number).await {
            Ok(Some(identity)) => Some(identity.safety_number),
            Ok(None) => None,
            Err(e) => {
                warn!(phone_number = %r.phone_number, error = %e, "Failed to fetch identity key");
                None
            }
        };

        bots.push(BotInfo {
            username,
            phone_number: r.phone_number.clone(),
            signal_link,
            registered_at: r.registered_at.to_rfc3339(),
            model: r.model.clone(),
            description,
            system_prompt: r.system_prompt.clone(),
            identity_key,
        });
    }

    Json(bots)
}
