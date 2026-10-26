//! Optional: OpenAI's real spend via the Admin API (`/v1/organization/costs`), for the project that
//! owns the saved OpenAI key. Needs a separate admin key (`sk-admin-…`); only called from the cost panel.

use serde::{Deserialize, Serialize};
use serde_json::Value;

const BASE: &str = "https://api.openai.com/v1/organization";

#[derive(Serialize)]
pub struct RealCost {
    pub usd: f64,
    /// None when the saved key's project couldn't be found, so `usd` is the whole organisation.
    pub project: Option<String>,
}

/// "sk-abc...xyz" matches a key that starts with "sk-abc" and ends with "xyz".
fn redacted_matches(redacted: &str, key: &str) -> bool {
    let head = redacted.split(['.', '*']).next().unwrap_or("");
    let tail = redacted.rsplit(['.', '*']).next().unwrap_or("");
    !head.is_empty() && !tail.is_empty() && redacted.len() > head.len() + tail.len()
        && key.starts_with(head) && key.ends_with(tail)
}

#[derive(Deserialize)]
struct Page { data: Vec<Value>, #[serde(default)] has_more: bool, #[serde(default)] next_page: Option<String>, #[serde(default)] last_id: Option<String> }

async fn get(http: &reqwest::Client, admin: &str, url: &str, q: &[(&str, String)]) -> Result<Page, String> {
    let r = http.get(url).bearer_auth(admin).query(q).send().await.map_err(|e| format!("Couldn't reach OpenAI: {e}"))?;
    match r.status().as_u16() {
        200 => r.json().await.map_err(|e| format!("Unexpected reply from OpenAI: {e}")),
        401 | 403 => Err("OpenAI refused the admin key (it needs an Admin key, sk-admin-…).".into()),
        s => Err(format!("OpenAI returned {s}")),
    }
}

/// Project id whose key list contains `key`, checked by its redacted form.
async fn find_project(http: &reqwest::Client, admin: &str, key: &str) -> Result<Option<(String, String)>, String> {
    let mut after: Option<String> = None;
    loop {
        let mut q = vec![("limit", "100".to_string())];
        if let Some(a) = &after { q.push(("after", a.clone())); }
        let page = get(http, admin, &format!("{BASE}/projects"), &q).await?;
        for p in &page.data {
            let (Some(id), name) = (p["id"].as_str(), p["name"].as_str().unwrap_or("")) else { continue };
            // shortcut: first 100 keys per project only; page if someone has more.
            let keys = get(http, admin, &format!("{BASE}/projects/{id}/api_keys"), &[("limit", "100".into())]).await?;
            if keys.data.iter().any(|k| k["redacted_value"].as_str().is_some_and(|r| redacted_matches(r, key))) {
                return Ok(Some((id.to_string(), name.to_string())));
            }
        }
        if !page.has_more { return Ok(None); }
        after = page.last_id.or_else(|| page.data.last().and_then(|p| p["id"].as_str()).map(String::from));
        if after.is_none() { return Ok(None); }
    }
}

/// Real USD spend since `since_s` (Unix seconds).
pub async fn since(http: &reqwest::Client, admin: &str, key: Option<&str>, since_s: i64) -> Result<RealCost, String> {
    let project = match key { Some(k) => find_project(http, admin, k).await?, None => None };
    let mut usd = 0.0;
    let mut page: Option<String> = None;
    loop {
        let mut q = vec![("start_time", since_s.to_string()), ("limit", "180".into())];
        if let Some((id, _)) = &project { q.push(("project_ids", id.clone())); }
        if let Some(p) = &page { q.push(("page", p.clone())); }
        let r = get(http, admin, &format!("{BASE}/costs"), &q).await?;
        usd += r.data.iter().flat_map(|b| b["results"].as_array().into_iter().flatten())
            .filter_map(|x| x["amount"]["value"].as_f64().or_else(|| x["amount"]["value"].as_str()?.parse().ok()))
            .sum::<f64>();
        match r.next_page { Some(p) if r.has_more => page = Some(p), _ => break }
    }
    Ok(RealCost { usd, project: project.map(|(id, name)| if name.is_empty() { id } else { name }) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_redacted() {
        assert!(redacted_matches("sk-abc...def", "sk-abc123def"));
        assert!(redacted_matches("sk-proj-***wxyz", "sk-proj-AAAAwxyz"));
        assert!(!redacted_matches("sk-abc...def", "sk-abc123xyz"));
        assert!(!redacted_matches("sk-abc", "sk-abc"));
    }
}
