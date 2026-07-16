//! Convert a `usage::Spec` into a JSON Schema (for the MCP tool `inputSchema`)
//! and, in reverse, convert a JSON argument object back into an argv vector that
//! `usage::Parser` can consume — so the CLI and MCP paths share one runner.

use heck::ToSnakeCase;
use serde_json::{json, Map, Value};
use usage::{Spec, SpecArg, SpecFlag};

/// JSON property name for an argument (matches the `usage_<name>` env convention).
fn arg_key(a: &SpecArg) -> String {
    a.name.to_snake_case()
}
fn flag_key(f: &SpecFlag) -> String {
    f.name.to_snake_case()
}

/// Build a JSON Schema object describing a jig's inputs.
pub fn to_input_schema(spec: &Spec) -> Map<String, Value> {
    let mut properties = Map::new();
    let mut required: Vec<Value> = Vec::new();
    let cmd = &spec.cmd;

    for arg in &cmd.args {
        if arg.hide {
            continue;
        }
        let mut prop = Map::new();
        if arg.var {
            prop.insert("type".into(), json!("array"));
            prop.insert("items".into(), json!({"type": "string"}));
        } else {
            prop.insert("type".into(), json!("string"));
            if let Some(choices) = &arg.choices {
                prop.insert("enum".into(), json!(choices.choices));
            }
        }
        if let Some(help) = arg.help.as_ref().or(arg.help_long.as_ref()) {
            prop.insert("description".into(), json!(help));
        }
        if !arg.default.is_empty() && !arg.var {
            prop.insert("default".into(), json!(arg.default[0]));
        }
        if arg.required {
            required.push(json!(arg_key(arg)));
        }
        properties.insert(arg_key(arg), Value::Object(prop));
    }

    for flag in &cmd.flags {
        if flag.hide {
            continue;
        }
        let mut prop = Map::new();
        match () {
            _ if flag.arg.is_some() => {
                let inner = flag.arg.as_ref().unwrap();
                if flag.var {
                    prop.insert("type".into(), json!("array"));
                    prop.insert("items".into(), json!({"type": "string"}));
                } else {
                    prop.insert("type".into(), json!("string"));
                    if let Some(choices) = &inner.choices {
                        prop.insert("enum".into(), json!(choices.choices));
                    }
                }
                if !inner.default.is_empty() && !flag.var {
                    prop.insert("default".into(), json!(inner.default[0]));
                }
            }
            _ if flag.count => {
                prop.insert("type".into(), json!("integer"));
            }
            _ => {
                prop.insert("type".into(), json!("boolean"));
            }
        }
        if let Some(help) = flag.help.as_ref().or(flag.help_long.as_ref()) {
            prop.insert("description".into(), json!(help));
        }
        if flag.required {
            required.push(json!(flag_key(flag)));
        }
        properties.insert(flag_key(flag), Value::Object(prop));
    }

    let mut schema = Map::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    schema
}

/// Reconstruct an argv vector from a JSON argument object. Flags are emitted
/// before positionals so a value-flag never swallows a positional token.
pub fn json_args_to_argv(spec: &Spec, args: &Map<String, Value>) -> Vec<String> {
    let mut argv = Vec::new();
    let cmd = &spec.cmd;

    for flag in &cmd.flags {
        let Some(v) = args.get(&flag_key(flag)) else {
            continue;
        };
        let long = flag
            .long
            .first()
            .cloned()
            .unwrap_or_else(|| flag.name.clone());
        let dashed = format!("--{long}");
        if flag.arg.is_some() {
            match v {
                Value::Array(items) => {
                    for item in items {
                        argv.push(dashed.clone());
                        argv.push(scalar(item));
                    }
                }
                Value::Null => {}
                _ => {
                    argv.push(dashed.clone());
                    argv.push(scalar(v));
                }
            }
        } else if flag.count {
            let n = v.as_u64().unwrap_or(0);
            for _ in 0..n {
                argv.push(dashed.clone());
            }
        } else if v.as_bool().unwrap_or(false) {
            argv.push(dashed.clone());
        }
    }

    for arg in &cmd.args {
        let Some(v) = args.get(&arg_key(arg)) else {
            continue;
        };
        match v {
            Value::Array(items) => {
                for item in items {
                    argv.push(scalar(item));
                }
            }
            Value::Null => {}
            _ => argv.push(scalar(v)),
        }
    }

    argv
}

/// Render a JSON scalar as a plain string (no surrounding quotes).
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn spec_from(script: &str) -> Spec {
        let mut f = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        f.write_all(script.as_bytes()).unwrap();
        Spec::parse_script(f.path()).unwrap()
    }

    #[test]
    fn schema_has_arg_and_flag() {
        let spec = spec_from(
            "#!/usr/bin/env bash\n#USAGE arg \"<logfile>\" help=\"the log\"\n#USAGE flag \"--json\" help=\"emit json\"\n",
        );
        let schema = to_input_schema(&spec);
        let props = schema["properties"].as_object().unwrap();
        assert_eq!(props["logfile"]["type"], json!("string"));
        assert_eq!(props["json"]["type"], json!("boolean"));
        assert_eq!(schema["required"], json!(["logfile"]));
    }

    #[test]
    fn argv_roundtrip() {
        let spec =
            spec_from("#!/usr/bin/env bash\n#USAGE arg \"<logfile>\"\n#USAGE flag \"--json\"\n");
        let args: Map<String, Value> =
            serde_json::from_value(json!({"logfile": "a.log", "json": true})).unwrap();
        let argv = json_args_to_argv(&spec, &args);
        assert_eq!(argv, vec!["--json".to_string(), "a.log".to_string()]);
    }
}
