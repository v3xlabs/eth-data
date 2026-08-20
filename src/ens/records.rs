//! `ens_records`: the standard text records, read in one pass.

use alloy::ens::ProviderEnsExt;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ens::{check_name, normalize, require_ens},
    server::{EthData, error::ToolError},
};

/// ENSIP-5 keys people actually set. Anything else can be asked for by name.
const STANDARD_KEYS: &[&str] = &[
    "avatar",
    "description",
    "display",
    "email",
    "url",
    "com.twitter",
    "com.github",
    "com.discord",
    "org.telegram",
];

const MAX_EXTRA_KEYS: usize = 8;
const MAX_KEY_BYTES: usize = 128;
const MAX_VALUE_BYTES: usize = 4_096;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordsParams {
    /// A name from `network_list` whose endpoint serves Ethereum mainnet.
    pub network: String,
    /// An ENS name, for example `vitalik.eth`.
    pub name: String,
    /// Extra text record keys to read alongside the standard set.
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecordsResult {
    pub network: String,
    pub name: String,
    pub fully_normalized: bool,
    /// Only keys that are set. A key with no value is absent rather than empty.
    pub records: Vec<Record>,
    /// The raw `avatar` record, repeated here because it is the one every
    /// caller wants. Resolving it to a fetchable URL is `avatar_get`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Record {
    pub key: String,
    pub value: String,
    /// True when the value was longer than the limit and was cut.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[tool_router(router = ens_records_router, vis = "pub(crate)")]
impl EthData {
    /// Read the standard ENS text records for a name.
    #[tool(name = "ens_records")]
    pub async fn ens_records(
        &self,
        Parameters(params): Parameters<RecordsParams>,
    ) -> Result<Json<RecordsResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        require_ens(&ready, &params.network)?;

        let name = normalize(&params.name);
        check_name(&name)?;

        let extra = params.keys.unwrap_or_default();
        if extra.len() > MAX_EXTRA_KEYS {
            return Err(ToolError::invalid_input(format!(
                "At most {MAX_EXTRA_KEYS} extra keys are supported, received {}.",
                extra.len()
            )));
        }
        if let Some(long) = extra.iter().find(|key| key.len() > MAX_KEY_BYTES) {
            return Err(ToolError::invalid_input(format!(
                "Key {:?} exceeds {MAX_KEY_BYTES} bytes.",
                &long[..MAX_KEY_BYTES.min(long.len())]
            )));
        }

        let mut keys: Vec<String> = STANDARD_KEYS.iter().map(|key| key.to_string()).collect();
        for key in extra {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }

        let lookups = keys.iter().map(|key| {
            let provider = ready.provider.clone();
            let name = name.name.clone();
            let key = key.clone();
            async move {
                let value = provider.lookup_txt(&name, &key).await.ok();
                (key, value)
            }
        });

        let records: Vec<Record> = futures::future::join_all(lookups)
            .await
            .into_iter()
            .filter_map(|(key, value)| {
                let value = value?;
                if value.is_empty() {
                    return None;
                }
                let truncated = value.len() > MAX_VALUE_BYTES;
                Some(Record {
                    key,
                    value: if truncated {
                        value.chars().take(MAX_VALUE_BYTES).collect()
                    } else {
                        value
                    },
                    truncated,
                })
            })
            .collect();

        let avatar = records
            .iter()
            .find(|record| record.key == "avatar")
            .map(|record| record.value.clone());

        Ok(Json(RecordsResult {
            network: params.network,
            name: name.name,
            fully_normalized: name.fully_normalized,
            records,
            avatar,
        }))
    }
}
