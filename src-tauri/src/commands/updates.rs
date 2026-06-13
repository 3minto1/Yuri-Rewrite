use crate::ai::compact_error_body;
use crate::domain::{AppState, UpdateCheckResult, UpdateDownloadResult};
use crate::{sanitize_file_name, to_string, GITHUB_LATEST_RELEASE_URL, GITHUB_REPOSITORY_URL};
use reqwest::Client;
use std::{env, fs, path::PathBuf, process::Command};
use tauri::State;

#[tauri::command]
pub(crate) fn open_github_url() -> Result<(), String> {
    open_url_in_default_browser(GITHUB_REPOSITORY_URL)
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    state: State<'_, AppState>,
) -> Result<UpdateCheckResult, String> {
    fetch_latest_update(&state.client).await
}

#[tauri::command]
pub(crate) async fn download_latest_update(
    state: State<'_, AppState>,
) -> Result<UpdateDownloadResult, String> {
    let update = fetch_latest_update(&state.client).await?;
    let response = state
        .client
        .get(&update.asset_download_url)
        .header("User-Agent", "YuriRewrite")
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(to_string)?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes);
        return Err(format!(
            "下载失败 HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let output_dir = resolve_update_download_dir(&state)?;
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let output_path = output_dir.join(sanitize_file_name(&update.asset_name));
    fs::write(&output_path, bytes).map_err(to_string)?;
    Ok(UpdateDownloadResult {
        path: output_path.to_string_lossy().to_string(),
        version: update.latest_version,
    })
}

pub(crate) async fn fetch_latest_update(client: &Client) -> Result<UpdateCheckResult, String> {
    let response = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("User-Agent", "YuriRewrite")
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        let body = response.text().await.map_err(to_string)?;
        return Err(format!(
            "检查更新失败 HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let latest_tag = release_tag_from_url(&final_url)
        .ok_or_else(|| format!("无法从 GitHub 最新发布地址解析版本：{}", final_url))?;
    let latest_version = normalize_release_version(&latest_tag);
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let release_url = format!("{}/releases/tag/{}", GITHUB_REPOSITORY_URL, latest_tag);
    let asset_name = portable_zip_name(&latest_version);
    let asset_download_url = format!(
        "{}/releases/download/{}/{}",
        GITHUB_REPOSITORY_URL, latest_tag, asset_name
    );

    Ok(UpdateCheckResult {
        current_version: current_version.clone(),
        latest_version: latest_version.clone(),
        latest_tag,
        is_latest: !is_newer_version(&latest_version, &current_version),
        release_url,
        asset_name,
        asset_download_url,
    })
}

pub(crate) fn release_tag_from_url(url: &str) -> Option<String> {
    let clean_url = url.split(['?', '#']).next().unwrap_or(url);
    let segments = clean_url
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    segments
        .windows(2)
        .find(|pair| pair[0] == "tag")
        .map(|pair| pair[1].to_string())
}

pub(crate) fn portable_zip_name(version: &str) -> String {
    format!(
        "YuriRewrite-v{}-windows-x64.zip",
        normalize_release_version(version)
    )
}

pub(crate) fn normalize_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    let candidate_parts = version_number_parts(candidate);
    let current_parts = version_number_parts(current);
    let max_len = candidate_parts.len().max(current_parts.len()).max(1);
    for idx in 0..max_len {
        let left = *candidate_parts.get(idx).unwrap_or(&0);
        let right = *current_parts.get(idx).unwrap_or(&0);
        if left != right {
            return left > right;
        }
    }
    false
}

pub(crate) fn version_number_parts(version: &str) -> Vec<u64> {
    normalize_release_version(version)
        .split(['.', '-', '+'])
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

pub(crate) fn resolve_update_download_dir(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(path) = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    Ok(default_download_dir().unwrap_or_else(|| state.data_dir.join("updates")))
}

pub(crate) fn default_download_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Downloads"))
        .filter(|path| path.exists())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Downloads"))
                .filter(|path| path.exists())
        })
}

pub(crate) fn open_url_in_default_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()
        .map_err(to_string)?;

    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status().map_err(to_string)?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(to_string)?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("无法打开链接：{}", url))
    }
}
