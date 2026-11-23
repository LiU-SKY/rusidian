//! Phones: vaults in shared storage (e.g. `Documents/MyVault`), so a sync
//! app such as Syncthing can keep them in step with the desktop. The vault
//! is still read with plain file paths, which needs Android's "All files
//! access"; the Kotlin side is `gen/android/.../StoragePlugin.kt`.

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Runtime};

/// Whether the app may read and write shared storage, and where it starts.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Access {
    pub granted: bool,
    /// `/storage/emulated/0` on most phones ("" on the desktop).
    pub root: String,
}

#[cfg(target_os = "android")]
struct Handle<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("storage")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("com.rusidian.app", "StoragePlugin")?;
                _app.manage(Handle(handle));
            }
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
async fn call<R: Runtime, T: serde::de::DeserializeOwned>(app: &AppHandle<R>, method: &str) -> Result<T, String> {
    use tauri::Manager;
    let handle = app.try_state::<Handle<R>>().ok_or("저장공간 플러그인이 없습니다")?;
    handle.0.run_mobile_plugin_async(method, ()).await.map_err(|e| e.to_string())
}

pub async fn access<R: Runtime>(_app: &AppHandle<R>) -> Result<Access, String> {
    #[cfg(target_os = "android")]
    return call(_app, "status").await;
    #[cfg(not(target_os = "android"))]
    Ok(Access { granted: true, root: String::new() })
}

/// Ask for access (opens the system settings page on Android 11+).
pub async fn request_access<R: Runtime>(_app: &AppHandle<R>) -> Result<Access, String> {
    #[cfg(target_os = "android")]
    return call(_app, "requestAccess").await;
    #[cfg(not(target_os = "android"))]
    Ok(Access { granted: true, root: String::new() })
}

/// The system folder picker; the chosen folder's path, `None` if cancelled.
pub async fn pick_folder<R: Runtime>(_app: &AppHandle<R>) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        #[derive(Deserialize)]
        struct Picked {
            path: Option<String>,
        }
        let picked: Picked = call(_app, "pickFolder").await?;
        return Ok(picked.path);
    }
    #[cfg(not(target_os = "android"))]
    Err("이 기기에서는 쓰지 않는 기능입니다".into())
}
