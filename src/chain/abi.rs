//! `calldata_encode` and `abi_decode`, in process.
//!
//! The Python server shelled out to `cast calldata` and `cast abi-decode` for
//! these. Doing it with `alloy-dyn-abi` removes a subprocess, a timeout, and a
//! megabyte of possible stdout from a code path that never needed a network.

use alloy::{
    dyn_abi::{DynSolType, DynSolValue, Specifier},
    json_abi::Function,
    primitives::hex,
};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    limits::{MAX_ABI_ARGUMENTS, check_signature, parse_bytes},
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EncodeParams {
    /// A Solidity function signature, for example
    /// `transfer(address,uint256)`.
    pub signature: String,
    /// One string per parameter, in order. Numbers may be decimal or
    /// 0x-prefixed; addresses and bytes are 0x-prefixed; booleans are
    /// `true` or `false`; arrays are `[a,b,c]`.
    pub arguments: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DecodeParams {
    /// A function signature such as `balanceOf(address)`, or a bare type list
    /// such as `(uint256,address)`.
    pub signature: String,
    /// The 0x-prefixed data to decode.
    pub data: String,
    /// Decode the function's inputs rather than its outputs. Set this when
    /// `data` is transaction calldata. Defaults to false.
    pub input_data: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EncodeResult {
    pub signature: String,
    pub selector: String,
    pub calldata: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DecodeResult {
    pub signature: String,
    /// One entry per decoded value, in order.
    pub values: Vec<DecodedValue>,
    /// Set when `input_data` was true and the data carried a selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Present when the data's selector does not match the signature given.
    /// The values below were still decoded, and are probably wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector_mismatch: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DecodedValue {
    /// The Solidity type, as resolved from the signature.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub value: Value,
}

/// Unbounded integers become decimal strings; everything else takes its natural
/// JSON shape. A `uint256` in a JSON number would silently lose precision.
fn to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Bool(inner) => json!(inner),
        DynSolValue::Int(inner, _) => json!(inner.to_string()),
        DynSolValue::Uint(inner, _) => json!(inner.to_string()),
        DynSolValue::FixedBytes(inner, size) => {
            json!(format!("0x{}", hex::encode(&inner[..*size])))
        }
        DynSolValue::Address(inner) => json!(inner.to_string()),
        DynSolValue::Function(inner) => json!(inner.to_string()),
        DynSolValue::Bytes(inner) => json!(format!("0x{}", hex::encode(inner))),
        DynSolValue::String(inner) => json!(inner),
        DynSolValue::Array(items) | DynSolValue::FixedArray(items) | DynSolValue::Tuple(items) => {
            Value::Array(items.iter().map(to_json).collect())
        }
    }
}

fn kind_of(value: &DynSolValue) -> String {
    value
        .as_type()
        .map(|kind| kind.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_function(signature: &str) -> Result<Function, ToolError> {
    Function::parse(signature).map_err(|error| {
        ToolError::invalid_input(format!(
            "Could not parse {signature:?} as a function: {error}"
        ))
    })
}

fn resolve_types(params: &[alloy::json_abi::Param]) -> Result<Vec<DynSolType>, ToolError> {
    params
        .iter()
        .map(|param| {
            param.resolve().map_err(|error| {
                ToolError::invalid_input(format!(
                    "Unsupported parameter type {:?}: {error}",
                    param.ty
                ))
            })
        })
        .collect()
}

fn decode_tuple(types: &[DynSolType], data: &[u8]) -> Result<Vec<DynSolValue>, ToolError> {
    if types.is_empty() {
        return Ok(Vec::new());
    }
    let tuple = DynSolType::Tuple(types.to_vec());
    match tuple.abi_decode_params(data) {
        Ok(DynSolValue::Tuple(values)) => Ok(values),
        Ok(other) => Ok(vec![other]),
        Err(error) => Err(ToolError::invalid_input(format!(
            "Could not decode {} bytes as ({}): {error}",
            data.len(),
            types
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ))),
    }
}

#[tool_router(router = abi_router, vis = "pub(crate)")]
impl EthData {
    /// Encode a function signature and its arguments into calldata. Runs
    /// locally against no network.
    #[tool(name = "calldata_encode")]
    pub async fn calldata_encode(
        &self,
        Parameters(params): Parameters<EncodeParams>,
    ) -> Result<Json<EncodeResult>, ToolError> {
        check_signature(&params.signature)?;
        let function = parse_function(&params.signature)?;
        let types = resolve_types(&function.inputs)?;
        let arguments = params.arguments.unwrap_or_default();

        if arguments.len() > MAX_ABI_ARGUMENTS {
            return Err(ToolError::invalid_input(format!(
                "At most {MAX_ABI_ARGUMENTS} arguments are supported."
            )));
        }
        if arguments.len() != types.len() {
            return Err(ToolError::invalid_input(format!(
                "{} takes {} argument(s), received {}.",
                function.name,
                types.len(),
                arguments.len()
            )));
        }

        let values = types
            .iter()
            .zip(&arguments)
            .enumerate()
            .map(|(index, (kind, argument))| {
                kind.coerce_str(argument).map_err(|error| {
                    ToolError::invalid_input(format!(
                        "Argument {index} ({argument:?}) is not a valid {kind}: {error}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let selector = function.selector();
        let encoded = DynSolValue::Tuple(values).abi_encode_params();
        let mut calldata = selector.to_vec();
        calldata.extend_from_slice(&encoded);

        Ok(Json(EncodeResult {
            signature: function.signature(),
            selector: format!("0x{}", hex::encode(selector)),
            calldata: format!("0x{}", hex::encode(calldata)),
        }))
    }

    /// Decode ABI data against a signature. Runs locally against no network.
    #[tool(name = "abi_decode")]
    pub async fn abi_decode(
        &self,
        Parameters(params): Parameters<DecodeParams>,
    ) -> Result<Json<DecodeResult>, ToolError> {
        check_signature(&params.signature)?;
        let data = parse_bytes(&params.data, "data")?;
        let input_data = params.input_data.unwrap_or(false);

        // A bare type list such as `(uint256,address)` is not a function, and is
        // what a caller reaches for when they know the shape but not the name.
        if Function::parse(&params.signature).is_err()
            && let Ok(DynSolType::Tuple(types)) = DynSolType::parse(&params.signature)
        {
            let values = decode_tuple(&types, &data)?;
            return Ok(Json(DecodeResult {
                signature: params.signature,
                values: values
                    .iter()
                    .map(|value| DecodedValue {
                        kind: kind_of(value),
                        name: None,
                        value: to_json(value),
                    })
                    .collect(),
                selector: None,
                selector_mismatch: None,
            }));
        }

        let function = parse_function(&params.signature)?;
        let params_meta = if input_data {
            &function.inputs
        } else {
            &function.outputs
        };
        let types = resolve_types(params_meta)?;

        let expected = function.selector();
        let mut selector = None;
        let mut selector_mismatch = None;
        let mut body = data.as_ref();

        if input_data && body.len() >= 4 {
            let found = &body[..4];
            selector = Some(format!("0x{}", hex::encode(found)));
            if found != expected.as_slice() {
                selector_mismatch = Some(format!(
                    "The data starts with 0x{}, but {} is 0x{}.",
                    hex::encode(found),
                    function.signature(),
                    hex::encode(expected)
                ));
            }
            body = &body[4..];
        }

        let values = decode_tuple(&types, body)?;

        Ok(Json(DecodeResult {
            signature: function.signature(),
            values: values
                .iter()
                .zip(params_meta.iter())
                .map(|(value, meta)| DecodedValue {
                    kind: kind_of(value),
                    name: (!meta.name.is_empty()).then(|| meta.name.clone()),
                    value: to_json(value),
                })
                .collect(),
            selector,
            selector_mismatch,
        }))
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, U256};

    use super::*;

    #[test]
    fn uints_render_as_decimal_strings() {
        let value = DynSolValue::Uint(U256::from(u128::MAX), 256);
        assert_eq!(to_json(&value), json!(u128::MAX.to_string()));
    }

    #[test]
    fn addresses_render_checksummed() {
        let address: Address = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
            .parse()
            .unwrap();
        assert_eq!(
            to_json(&DynSolValue::Address(address)),
            json!("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
        );
    }

    #[test]
    fn tuples_render_as_arrays() {
        let value = DynSolValue::Tuple(vec![
            DynSolValue::Bool(true),
            DynSolValue::Uint(U256::from(7u64), 256),
        ]);
        assert_eq!(to_json(&value), json!([true, "7"]));
    }
}
