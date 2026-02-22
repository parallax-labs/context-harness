use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::config::{Config, S3ConnectorConfig};
use crate::models::SourceItem;

type HmacSha256 = Hmac<Sha256>;

/// Scan an S3 bucket and produce SourceItems.
///
/// Uses the S3 REST API directly with AWS SigV4 signing.
/// Credentials are read from environment variables:
///   AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, (optional) AWS_SESSION_TOKEN
pub async fn scan_s3(config: &Config) -> Result<Vec<SourceItem>> {
    let s3_config = config
        .connectors
        .s3
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("S3 connector not configured"))?;

    let creds = AwsCredentials::from_env()?;

    // Build glob sets
    let include_set = build_globset(&s3_config.include_globs)?;

    let mut default_excludes = vec![
        "**/.git/**".to_string(),
        "**/node_modules/**".to_string(),
    ];
    default_excludes.extend(s3_config.exclude_globs.clone());
    let exclude_set = build_globset(&default_excludes)?;

    // List all objects
    let objects = list_objects(s3_config, &creds).await?;

    let mut items = Vec::new();
    let client = reqwest::Client::new();

    for obj in &objects {
        // Compute relative key (strip prefix for glob matching)
        let rel_key = if s3_config.prefix.is_empty() {
            obj.key.clone()
        } else {
            let prefix = s3_config.prefix.trim_end_matches('/');
            obj.key
                .strip_prefix(prefix)
                .map(|s| s.trim_start_matches('/').to_string())
                .unwrap_or_else(|| obj.key.clone())
        };

        // Apply glob filters
        if exclude_set.is_match(&rel_key) {
            continue;
        }
        if !include_set.is_match(&rel_key) {
            continue;
        }

        // Download the object
        let body = match download_object(s3_config, &creds, &client, &obj.key).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "Warning: failed to download s3://{}/{}: {}",
                    s3_config.bucket, obj.key, e
                );
                continue;
            }
        };

        let title = obj.key.rsplit('/').next().unwrap_or(&obj.key).to_string();
        let source_url = format!("s3://{}/{}", s3_config.bucket, obj.key);

        let metadata = serde_json::json!({
            "bucket": s3_config.bucket,
            "etag": obj.etag,
            "size": obj.size,
        });

        items.push(SourceItem {
            source: "s3".to_string(),
            source_id: obj.key.clone(),
            source_url: Some(source_url),
            title: Some(title),
            author: None,
            created_at: Utc.timestamp_opt(obj.last_modified, 0).unwrap(),
            updated_at: Utc.timestamp_opt(obj.last_modified, 0).unwrap(),
            content_type: detect_content_type(&obj.key),
            body,
            metadata_json: metadata.to_string(),
            raw_json: None,
        });
    }

    items.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    Ok(items)
}

// ============ AWS Credentials ============

struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl AwsCredentials {
    fn from_env() -> Result<Self> {
        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID")
            .context("AWS_ACCESS_KEY_ID environment variable not set")?;
        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .context("AWS_SECRET_ACCESS_KEY environment variable not set")?;
        let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

// ============ S3 Object Listing ============

struct S3Object {
    key: String,
    last_modified: i64,
    etag: String,
    size: i64,
}

async fn list_objects(
    s3_config: &S3ConnectorConfig,
    creds: &AwsCredentials,
) -> Result<Vec<S3Object>> {
    let client = reqwest::Client::new();
    let mut objects = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut query_params = vec![
            ("list-type".to_string(), "2".to_string()),
            ("max-keys".to_string(), "1000".to_string()),
        ];

        if !s3_config.prefix.is_empty() {
            query_params.push(("prefix".to_string(), s3_config.prefix.clone()));
        }

        if let Some(ref token) = continuation_token {
            query_params.push(("continuation-token".to_string(), token.clone()));
        }

        let host = s3_host(s3_config);
        let url = format!("https://{}/", host);

        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // Build canonical query string (must be sorted)
        let mut sorted_params = query_params.clone();
        sorted_params.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_querystring: String = sorted_params
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let payload_hash = hex_sha256(b"");

        let mut headers = vec![
            ("host".to_string(), host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        if let Some(ref token) = creds.session_token {
            headers.push(("x-amz-security-token".to_string(), token.clone()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let signed_headers: String = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v))
            .collect();

        let canonical_request = format!(
            "GET\n/\n{}\n{}\n{}\n{}",
            canonical_querystring, canonical_headers, signed_headers, payload_hash
        );

        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, s3_config.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            hex_sha256(canonical_request.as_bytes())
        );

        let signing_key = derive_signing_key(
            &creds.secret_access_key,
            &date_stamp,
            &s3_config.region,
            "s3",
        );
        let signature = hex_hmac_sha256(&signing_key, string_to_sign.as_bytes());

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            creds.access_key_id, credential_scope, signed_headers, signature
        );

        let full_url = format!("{}?{}", url, canonical_querystring);

        let mut req_builder = client
            .get(&full_url)
            .header("Authorization", &authorization)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date);

        if let Some(ref token) = creds.session_token {
            req_builder = req_builder.header("x-amz-security-token", token);
        }

        let resp = req_builder.send().await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to list S3 objects in s3://{}/{}: {}",
                s3_config.bucket,
                s3_config.prefix,
                e
            )
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "S3 ListObjectsV2 failed (HTTP {}): {}",
                status,
                body.chars().take(500).collect::<String>()
            );
        }

        let xml_body = resp.text().await?;
        let (batch, is_truncated, next_token) = parse_list_objects_response(&xml_body)?;
        objects.extend(batch);

        if is_truncated {
            continuation_token = next_token;
        } else {
            break;
        }
    }

    Ok(objects)
}

async fn download_object(
    s3_config: &S3ConnectorConfig,
    creds: &AwsCredentials,
    client: &reqwest::Client,
    key: &str,
) -> Result<String> {
    let host = s3_host(s3_config);
    let encoded_key = key
        .split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("https://{}/{}", host, encoded_key);

    let now = Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

    let payload_hash = hex_sha256(b"");

    let mut headers = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    if let Some(ref token) = creds.session_token {
        headers.push(("x-amz-security-token".to_string(), token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: String = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v))
        .collect();

    let canonical_uri = format!("/{}", encoded_key);
    let canonical_request = format!(
        "GET\n{}\n\n{}\n{}\n{}",
        canonical_uri, canonical_headers, signed_headers, payload_hash
    );

    let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, s3_config.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        credential_scope,
        hex_sha256(canonical_request.as_bytes())
    );

    let signing_key = derive_signing_key(
        &creds.secret_access_key,
        &date_stamp,
        &s3_config.region,
        "s3",
    );
    let signature = hex_hmac_sha256(&signing_key, string_to_sign.as_bytes());

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        creds.access_key_id, credential_scope, signed_headers, signature
    );

    let mut req_builder = client
        .get(&url)
        .header("Authorization", &authorization)
        .header("x-amz-content-sha256", &payload_hash)
        .header("x-amz-date", &amz_date);

    if let Some(ref token) = creds.session_token {
        req_builder = req_builder.header("x-amz-security-token", token);
    }

    let resp = req_builder.send().await.map_err(|e| {
        anyhow::anyhow!("Failed to get s3://{}/{}: {}", s3_config.bucket, key, e)
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        bail!("S3 GetObject failed (HTTP {}) for key '{}'", status, key);
    }

    let bytes = resp.bytes().await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

// ============ AWS SigV4 Helpers ============

fn s3_host(s3_config: &S3ConnectorConfig) -> String {
    if let Some(ref endpoint) = s3_config.endpoint_url {
        // Custom endpoint (MinIO, LocalStack, etc.)
        endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    } else {
        format!("{}.s3.{}.amazonaws.com", s3_config.bucket, s3_config.region)
    }
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex_hmac_sha256(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

fn derive_signing_key(secret_key: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn uri_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

// ============ XML Parsing (minimal, no extra deps) ============

fn parse_list_objects_response(xml: &str) -> Result<(Vec<S3Object>, bool, Option<String>)> {
    let mut objects = Vec::new();
    let is_truncated = extract_xml_value(xml, "IsTruncated")
        .map(|v| v == "true")
        .unwrap_or(false);
    let next_token = extract_xml_value(xml, "NextContinuationToken");

    // Parse <Contents> blocks
    let mut remaining = xml;
    while let Some(start) = remaining.find("<Contents>") {
        let block_start = start + "<Contents>".len();
        if let Some(end) = remaining[block_start..].find("</Contents>") {
            let block = &remaining[block_start..block_start + end];

            let key = extract_xml_value(block, "Key").unwrap_or_default();
            if key.is_empty() || key.ends_with('/') {
                remaining = &remaining[block_start + end + "</Contents>".len()..];
                continue;
            }

            let last_modified = extract_xml_value(block, "LastModified")
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            let etag = extract_xml_value(block, "ETag")
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();

            let size = extract_xml_value(block, "Size")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            objects.push(S3Object {
                key,
                last_modified,
                etag,
                size,
            });

            remaining = &remaining[block_start + end + "</Contents>".len()..];
        } else {
            break;
        }
    }

    Ok((objects, is_truncated, next_token))
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = xml.find(&open) {
        let value_start = start + open.len();
        if let Some(end) = xml[value_start..].find(&close) {
            return Some(xml[value_start..value_start + end].to_string());
        }
    }
    None
}

fn detect_content_type(key: &str) -> String {
    match key.rsplit('.').next() {
        Some("md") => "text/markdown".to_string(),
        Some("txt") => "text/plain".to_string(),
        Some("json") => "application/json".to_string(),
        Some("yaml" | "yml") => "text/yaml".to_string(),
        Some("rst") => "text/x-rst".to_string(),
        Some("html" | "htm") => "text/html".to_string(),
        _ => "text/plain".to_string(),
    }
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}
