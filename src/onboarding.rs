use log::info;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use serde_json::json;
use starknet::core::{crypto::compute_hash_on_elements, types::TypedData, utils::starknet_keccak};
use starknet_crypto::Felt;
use starknet_signers::SigningKey;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ParadexConfig {
    pub starknet_chain_id: String,
}

impl ParadexConfig {
    pub fn testnet() -> Self {
        Self {
            starknet_chain_id: "SN_GOERLI".to_string(),
        }
    }

    pub fn production() -> Self {
        Self {
            starknet_chain_id: "SN_MAIN".to_string(),
        }
    }
}

/// 将字符串转换为 felt（0x 前缀的十六进制表示）
fn string_to_felt_hex(s: &str) -> String {
    if s.is_empty() {
        return "0x0".to_string();
    }

    let mut result = String::from("0x");
    for byte in s.as_bytes() {
        result.push_str(&format!("{:02x}", byte));
    }
    result
}

/// 构建 Paradex onboarding TypedData (完全匹配 Python 实现)
fn build_onboarding_typed_data(chain_id: &str) -> TypedData {
    let typed_data_json = json!({
        "types": {
            "StarkNetDomain": [
                { "name": "name", "type": "felt" },
                { "name": "version", "type": "felt" },
                { "name": "chainId", "type": "felt" }
            ],
            "Constant": [
                { "name": "action", "type": "felt" }
            ]
        },
        "primaryType": "Constant",
        "domain": {
            "name": string_to_felt_hex("Paradex"),
            "chainId": string_to_felt_hex(chain_id),
            "version": "1"
        },
        "message": {
            "action": "Onboarding"
        }
    });

    serde_json::from_value(typed_data_json).expect("Failed to parse TypedData")
}

/// 构建 Paradex auth TypedData (完全匹配 Python 实现)
fn build_auth_typed_data(chain_id: &str, timestamp: u64, expiry: u64) -> TypedData {
    let typed_data_json = json!({
        "types": {
            "StarkNetDomain": [
                { "name": "name", "type": "felt" },
                { "name": "version", "type": "felt" },
                { "name": "chainId", "type": "felt" }
            ],
            "Request": [
                { "name": "method", "type": "felt" },
                { "name": "path", "type": "felt" },
                { "name": "body", "type": "felt" },
                { "name": "timestamp", "type": "felt" },
                { "name": "expiration", "type": "felt" }
            ]
        },
        "primaryType": "Request",
        "domain": {
            "name": string_to_felt_hex("Paradex"),
            "chainId": string_to_felt_hex(chain_id),
            "version": "1"
        },
        "message": {
            "method": "POST",
            "path": "/v1/auth",
            "body": "",
            "timestamp": timestamp,
            "expiration": expiry
        }
    });

    serde_json::from_value(typed_data_json).expect("Failed to parse TypedData")
}

/// 执行 onboarding
pub async fn perform_onboarding(
    http_client: &HttpClient,
    base_url: &str,
    account_address: &str,
    private_key: &str,
    ethereum_account: &str,
    config: &ParadexConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // 解析私钥
    let private_key_felt =
        Felt::from_hex(private_key).map_err(|e| format!("Failed to parse private key: {}", e))?;
    let signing_key = SigningKey::from_secret_scalar(private_key_felt);

    // 获取公钥并构建签名
    let public_key = signing_key.verifying_key().scalar();
    let typed_data = build_onboarding_typed_data(&config.starknet_chain_id);
    let account_felt = Felt::from_hex(account_address)
        .map_err(|e| format!("Failed to parse account address: {}", e))?;
    let domain_hash = typed_data.encoder().domain().encoded_hash();
    let message_struct_hash = typed_data
        .encoder()
        .encode_value(typed_data.primary_type(), typed_data.message())
        .map_err(|e| format!("Failed to encode message struct: {}", e))?;
    let domain = typed_data.encoder().domain();
    let domain_type_hash = starknet_keccak(b"StarkNetDomain(name:felt,version:felt,chainId:felt)");
    let manual_domain_hash = compute_hash_on_elements(&[
        domain_type_hash,
        domain.name,
        domain.version,
        domain.chain_id,
    ]);
    info!(
        "Onboarding typed data JSON: {}",
        serde_json::to_string(&typed_data).unwrap_or_default()
    );
    info!("Onboarding domain type hash: 0x{:x}", domain_type_hash);
    info!(
        "Onboarding domain fields name=0x{:x}, version=0x{:x}, chain_id=0x{:x}",
        domain.name, domain.version, domain.chain_id
    );
    info!(
        "Onboarding domain_hash=0x{:x}, manual_domain_hash=0x{:x}, message_struct_hash=0x{:x}",
        domain_hash, manual_domain_hash, message_struct_hash
    );
    let message_hash = typed_data
        .message_hash(account_felt)
        .map_err(|e| format!("Failed to encode TypedData: {}", e))?;
    info!(
        "Onboarding typed data revision {:?}, message hash: 0x{:x}",
        typed_data.revision(),
        message_hash
    );
    let signature = signing_key.sign(&message_hash)?;

    // 发送 onboarding 请求
    let signature_header = format!(r#"["{}","{}"]"#, signature.r, signature.s);
    let url = format!("{}/onboarding", base_url);

    info!("POST {} with StarkNet account: {}", url, account_address);

    let response = http_client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("PARADEX-ETHEREUM-ACCOUNT", ethereum_account)
        .header("PARADEX-STARKNET-ACCOUNT", account_address)
        .header("PARADEX-STARKNET-SIGNATURE", &signature_header)
        .json(&json!({"public_key": format!("0x{:x}", public_key)}))
        .send()
        .await?;

    if response.status().is_success() {
        info!("Onboarding successful");
        Ok(())
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Onboarding failed: {}", error_text).into())
    }
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    jwt_token: String,
}

/// 获取 JWT token
pub async fn get_jwt_token(
    http_client: &HttpClient,
    base_url: &str,
    account_address: &str,
    private_key: &str,
    config: &ParadexConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    let private_key_felt =
        Felt::from_hex(private_key).map_err(|e| format!("Failed to parse private key: {}", e))?;
    let signing_key = SigningKey::from_secret_scalar(private_key_felt);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expiry = now + 24 * 60 * 60;

    // 构建并签名 TypedData
    let typed_data = build_auth_typed_data(&config.starknet_chain_id, now, expiry);
    let account_felt = Felt::from_hex(account_address)
        .map_err(|e| format!("Failed to parse account address: {}", e))?;
    let domain_hash = typed_data.encoder().domain().encoded_hash();
    let message_struct_hash = typed_data
        .encoder()
        .encode_value(typed_data.primary_type(), typed_data.message())
        .map_err(|e| format!("Failed to encode message struct: {}", e))?;
    let domain = typed_data.encoder().domain();
    let domain_type_hash = starknet_keccak(b"StarkNetDomain(name:felt,version:felt,chainId:felt)");
    let manual_domain_hash = compute_hash_on_elements(&[
        domain_type_hash,
        domain.name,
        domain.version,
        domain.chain_id,
    ]);
    info!(
        "Auth typed data JSON: {}",
        serde_json::to_string(&typed_data).unwrap_or_default()
    );
    info!("Auth domain type hash: 0x{:x}", domain_type_hash);
    info!(
        "Auth domain fields name=0x{:x}, version=0x{:x}, chain_id=0x{:x}",
        domain.name, domain.version, domain.chain_id
    );
    info!(
        "Auth domain_hash=0x{:x}, manual_domain_hash=0x{:x}, message_struct_hash=0x{:x}",
        domain_hash, manual_domain_hash, message_struct_hash
    );
    let message_hash = typed_data
        .message_hash(account_felt)
        .map_err(|e| format!("Failed to encode TypedData: {}", e))?;
    info!(
        "Auth typed data revision {:?}, message hash: 0x{:x}",
        typed_data.revision(),
        message_hash
    );
    let signature = signing_key.sign(&message_hash)?;

    // 发送认证请求
    let signature_header = format!(r#"["{}","{}"]"#, signature.r, signature.s);
    let url = format!("{}/auth", base_url);

    info!("POST {} with StarkNet account: {}", url, account_address);

    let response = http_client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("PARADEX-STARKNET-ACCOUNT", account_address)
        .header("PARADEX-STARKNET-SIGNATURE", &signature_header)
        .header("PARADEX-TIMESTAMP", now.to_string())
        .header("PARADEX-SIGNATURE-EXPIRATION", expiry.to_string())
        .send()
        .await?;

    if response.status().is_success() {
        let auth_response: AuthResponse = response.json().await?;
        info!("JWT token obtained successfully");
        Ok(auth_response.jwt_token)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("JWT auth failed: {}", error_text).into())
    }
}
