//! Literal RPC annotation interpretation. Never evaluates project expressions.

use super::{RpcConfig, RpcMode, TransferMode};
use crate::syntax::{self, ast};

impl RpcConfig {
    /// Interprets `@rpc` argument source texts, including their quotes.
    ///
    /// Up to three string literals select distinct categories in any order;
    /// a fourth argument is the channel. Repeating a category is an error,
    /// even when both values are identical. Bare identifiers are expressions,
    /// not implicitly quoted strings.
    ///
    /// Supports String/StringName literals and signed decimal integer channels.
    /// Other expressions (including constants, arithmetic, float-to-int coercion
    /// and non-decimal integers) are explicitly unresolved, not evaluated or
    /// declared invalid GDScript. Defaults apply only to omitted arguments.
    pub fn from_arguments(arguments: &[&str]) -> Result<Self, String> {
        if arguments.len() > 4 {
            return Err("@rpc accepts at most 4 arguments".into());
        }
        let mut config = Self {
            mode: RpcMode::Authority,
            call_local: false,
            transfer: TransferMode::Unreliable,
            channel: 0,
        };
        let mut categories = [false; 3];
        for (index, argument) in arguments.iter().take(3).enumerate() {
            let text = string_argument(argument).ok_or_else(|| {
                format!(
                    "cannot resolve @rpc argument {}: expected a string literal, got {argument}",
                    index + 1
                )
            })?;
            let (category, name) = match text.as_str() {
                "authority" | "any_peer" => {
                    config.mode = if text == "authority" {
                        RpcMode::Authority
                    } else {
                        RpcMode::AnyPeer
                    };
                    (0, "permission")
                }
                "call_remote" | "call_local" => {
                    config.call_local = text == "call_local";
                    (1, "locality")
                }
                "unreliable" | "unreliable_ordered" | "reliable" => {
                    config.transfer = match text.as_str() {
                        "unreliable" => TransferMode::Unreliable,
                        "unreliable_ordered" => TransferMode::UnreliableOrdered,
                        _ => TransferMode::Reliable,
                    };
                    (2, "transfer mode")
                }
                _ => return Err(format!("unknown @rpc argument {}: {argument}", index + 1)),
            };
            if categories[category] {
                return Err(format!("@rpc {name} must be specified no more than once"));
            }
            categories[category] = true;
        }
        if let Some(argument) = arguments.get(3) {
            config.channel = argument.trim().parse::<i64>().map_err(|_| {
                format!(
                    "cannot resolve @rpc channel: expected a signed decimal integer literal, got {argument}"
                )
            })?;
        }
        Ok(config)
    }
}

/// Decode with the shared literal decoder, not quote trimming. Godot accepts
/// StringName literals here as well.
fn string_argument(argument: &str) -> Option<String> {
    let text = argument.trim();
    let parsed = syntax::parse(&format!("const __rpc_argument = {text}\n"));
    if !parsed.is_valid() {
        return None;
    }
    let file = ast::SourceFile::cast(parsed.root())?;
    let mut members = file.members();
    let ast::Member::Const(constant) = members.next()? else {
        return None;
    };
    if members.next().is_some() {
        return None;
    }
    let value = constant.initializer()?;
    // Reject trailing statements/comments rather than accepting a literal prefix.
    if value.trimmed_text() != text {
        return None;
    }
    ast::string_or_name_literal(value)
}
