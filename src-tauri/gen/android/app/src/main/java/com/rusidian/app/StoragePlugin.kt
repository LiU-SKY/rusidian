package com.rusidian.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.Settings
import androidx.activity.result.ActivityResult
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Vaults in shared storage (e.g. Documents/MyVault), read with plain file
 * paths like on the desktop. That needs "All files access"
 * (MANAGE_EXTERNAL_STORAGE); the system folder picker is used only to
 * choose the folder, whose path is then derived from its document id.
 */
@TauriPlugin(
  permissions = [
    // Android 10 and older have no All files access: the old storage permission.
    Permission(strings = [Manifest.permission.WRITE_EXTERNAL_STORAGE], alias = "legacy"),
  ]
)
class StoragePlugin(private val activity: Activity) : Plugin(activity) {

  private fun granted(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else getPermissionState("legacy") == PermissionState.GRANTED

  private fun status(): JSObject {
    val o = JSObject()
    o.put("granted", granted())
    o.put("root", Environment.getExternalStorageDirectory().absolutePath)
    return o
  }

  @Command
  fun status(invoke: Invoke) = invoke.resolve(status())

  /** Open the system page where the user turns on All files access for this app. */
  @Command
  fun requestAccess(invoke: Invoke) {
    if (granted()) {
      invoke.resolve(status())
      return
    }
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
      requestPermissionForAlias("legacy", invoke, "legacyResult")
      return
    }
    val intent = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION)
      .setData(Uri.parse("package:${activity.packageName}"))
    try {
      startActivityForResult(invoke, intent, "accessResult")
    } catch (e: Exception) {
      // Some devices only have the list of all apps.
      startActivityForResult(invoke, Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION), "accessResult")
    }
  }

  @ActivityCallback
  fun accessResult(invoke: Invoke, result: ActivityResult) = invoke.resolve(status())

  @PermissionCallback
  fun legacyResult(invoke: Invoke) = invoke.resolve(status())

  /** Let the user pick a folder; resolves with its file path (null if cancelled). */
  @Command
  fun pickFolder(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    startActivityForResult(invoke, intent, "folderResult")
  }

  @ActivityCallback
  fun folderResult(invoke: Invoke, result: ActivityResult) {
    val o = JSObject()
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      o.put("path", null)
      invoke.resolve(o)
      return
    }
    val path = treePath(uri)
    if (path == null) {
      invoke.reject("이 위치는 vault로 쓸 수 없습니다. 내부 저장공간이나 SD 카드의 폴더를 고르세요.")
      return
    }
    o.put("path", path)
    invoke.resolve(o)
  }

  /** "primary:Documents/Vault" → /storage/emulated/0/Documents/Vault, "1A2B-3C4D:Notes" → /storage/1A2B-3C4D/Notes. */
  private fun treePath(uri: Uri): String? {
    if (uri.authority != "com.android.externalstorage.documents") return null
    val id = DocumentsContract.getTreeDocumentId(uri)
    val colon = id.indexOf(':')
    if (colon < 0) return null
    val volume = id.substring(0, colon)
    val rel = id.substring(colon + 1).trimEnd('/')
    val base =
      if (volume == "primary") Environment.getExternalStorageDirectory().absolutePath
      else "/storage/$volume"
    return if (rel.isEmpty()) base else "$base/$rel"
  }
}
