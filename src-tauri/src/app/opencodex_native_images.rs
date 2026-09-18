//! Native image configuration for Manager-owned OSIR API-key routes.
use crate::errors::AppError;
use serde_json::{json, Map, Value};
use toml_edit::{value, DocumentMut};
use url::Url;

pub(super) const MODEL: &str = "gpt-image-2.5-flare";
pub(super) const MODEL_HEADER: &str = "X-Osir-Codex-Image-Model";
const ACTOR_HEADER: &str = "x-openai-actor-authorization";

fn keyed(provider: &Value) -> bool {
    provider["disabled"] != true
        && provider["adapter"] == "openai-responses"
        && provider
            .get("authMode")
            .and_then(Value::as_str)
            .is_none_or(|m| m == "key")
        && provider
            .get("apiKey")
            .and_then(Value::as_str)
            .is_some_and(|k| !k.trim().is_empty())
}

fn osir(provider: &Value) -> bool {
    keyed(provider)
        && provider
            .get("baseUrl")
            .and_then(Value::as_str)
            .and_then(|s| Url::parse(s).ok())
            .is_some_and(|u| {
                u.scheme() == "https"
                    && matches!(u.host_str(), Some("api.osirclaw.com" | "osirclaw.com"))
            })
        && provider
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| {
                models
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|m| m.starts_with("gpt-") && !m.starts_with("gpt-image-"))
            })
}

// Preserve an explicit custom binding, never silently switch billing providers.
pub(super) fn configure(config: &mut Map<String, Value>) -> Result<bool, AppError> {
    let before = config.clone();
    let providers = config.get("providers").and_then(Value::as_object);
    let selected = config
        .get("images")
        .and_then(|i| i.get("provider"))
        .and_then(Value::as_str);
    let candidate = match selected {
        Some(id) if providers.and_then(|p| p.get(id)).is_some_and(keyed) => {
            if !providers.and_then(|p| p.get(id)).is_some_and(osir) {
                return Ok(false);
            }
            Some(id.to_string())
        }
        Some(id) if id != "osirapi-openai" && id != "osir-gpt" => {
            return Err(AppError::Engine(
                "图片供应商不可用，请检查自定义图片路由；未自动更换计费渠道".into(),
            ))
        }
        _ => providers.and_then(|p| p.iter().find(|(_, v)| osir(v)).map(|(k, _)| k.clone())),
    };
    let Some(id) = candidate else {
        return Ok(false);
    };
    let images = config
        .entry("images")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| AppError::Engine("OpenCodex images 必须是对象".into()))?;
    images.insert("provider".into(), json!(id));
    let provider = config
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .and_then(|p| p.get_mut(&id))
        .unwrap();
    let headers = provider
        .as_object_mut()
        .unwrap()
        .entry("headers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| AppError::Engine("图片供应商 headers 必须是对象".into()))?;
    headers.retain(|key, _| !key.eq_ignore_ascii_case(MODEL_HEADER));
    headers.insert(MODEL_HEADER.into(), json!(MODEL));
    Ok(*config != before)
}

pub(super) fn enabled(config: &Map<String, Value>) -> bool {
    config
        .get("images")
        .and_then(|i| i.get("provider"))
        .and_then(Value::as_str)
        .and_then(|id| config.get("providers")?.get(id))
        .is_some_and(osir)
}

pub(super) fn configure_codex(
    document: &mut DocumentMut,
    provider_id: &str,
) -> Result<(), AppError> {
    if !document.contains_key("features") {
        document["features"] = toml_edit::table();
    }
    let features = document["features"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("features 必须是 TOML 表".into()))?;
    features["image_generation"] = value(true);
    let provider = document["model_providers"][provider_id]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("图片路由配置不存在".into()))?;
    if !provider.contains_key("http_headers") {
        provider["http_headers"] = value(toml_edit::InlineTable::new());
    }
    let headers = provider["http_headers"]
        .as_table_like_mut()
        .ok_or_else(|| AppError::Engine("http_headers 必须是 TOML 表".into()))?;
    let duplicates: Vec<String> = headers
        .iter()
        .filter(|(k, _)| {
            k.eq_ignore_ascii_case(ACTOR_HEADER) || k.eq_ignore_ascii_case(MODEL_HEADER)
        })
        .map(|(k, _)| k.to_string())
        .collect();
    for key in duplicates {
        headers.remove(&key);
    }
    headers.insert(ACTOR_HEADER, value("local-image-extension"));
    headers.insert(MODEL_HEADER, value(MODEL));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Map<String, Value> {
        json!({"providers":{"osirapi-openai":{"adapter":"openai-responses","baseUrl":"https://api.osirclaw.com/v1","apiKey":"test-old-key","authMode":"key","models":["gpt-6-astra"],"headers":{"Keep":"yes"}}}}).as_object().unwrap().clone()
    }
    #[test]
    fn native_images_bind_same_provider_and_preserve_fields() {
        let mut c = config();
        c.insert("images".into(), json!({"timeoutMs":90000}));
        assert!(configure(&mut c).unwrap());
        assert!(enabled(&c));
        assert_eq!(c["images"]["provider"], "osirapi-openai");
        assert_eq!(c["images"]["timeoutMs"], 90000);
        assert_eq!(
            c["providers"]["osirapi-openai"]["headers"][MODEL_HEADER],
            MODEL
        );
        assert_eq!(c["providers"]["osirapi-openai"]["headers"]["Keep"], "yes");
        assert!(!configure(&mut c).unwrap());
        c.get_mut("providers").unwrap()["osirapi-openai"]["apiKey"] = json!("test-new-key");
        assert!(!configure(&mut c).unwrap());
        assert!(!serde_json::to_string(&c).unwrap().contains("test-old-key"));
    }
    #[test]
    fn native_images_preserve_custom_binding_and_reject_invalid_one() {
        let mut c = config();
        c.get_mut("providers").unwrap()["custom"] = json!({"adapter":"openai-responses","baseUrl":"https://example.com/v1","apiKey":"custom-key"});
        c.insert("images".into(), json!({"provider":"custom"}));
        let before = c.clone();
        assert!(!configure(&mut c).unwrap());
        assert_eq!(c, before);
        assert!(!enabled(&c));
        c.get_mut("images").unwrap()["provider"] = json!("missing-custom");
        assert!(configure(&mut c).is_err());
    }
    #[test]
    fn native_images_ignore_ineligible_routes() {
        let mut c = config();
        c.get_mut("providers").unwrap()["osirapi-openai"]["disabled"] = json!(true);
        assert!(!configure(&mut c).unwrap());
        assert!(!enabled(&c));
    }
    #[test]
    fn native_images_upgrade_old_codex_without_losing_other_settings() {
        let mut d:DocumentMut="[features]\nimage_generation = false\ngoals = true\n[model_providers.opencodex]\nhttp_headers = { Keep = \"yes\" }\n".parse().unwrap();
        configure_codex(&mut d, "opencodex").unwrap();
        assert_eq!(d["features"]["image_generation"].as_bool(), Some(true));
        assert_eq!(d["features"]["goals"].as_bool(), Some(true));
        assert_eq!(
            d["model_providers"]["opencodex"]["http_headers"]["Keep"].as_str(),
            Some("yes")
        );
        let before = d.to_string();
        configure_codex(&mut d, "opencodex").unwrap();
        assert_eq!(before, d.to_string());
    }
}
