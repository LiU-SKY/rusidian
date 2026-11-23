<script lang="ts">
  import { onMount } from "svelte";
  import { deviceVault, pickVaultFolder, requestStorageAccess, storageAccess, type StorageAccess } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    /** A folder was chosen: open it as the vault. */
    onpick: (path: string) => void;
    onclose: () => void;
  }

  let { onpick, onclose }: Props = $props();

  let access = $state<StorageAccess | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  onMount(() => {
    storageAccess()
      .then((a) => (access = a))
      .catch((e) => (error = String(e)));
  });

  async function run(task: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await task();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const allow = () => run(async () => void (access = await requestStorageAccess()));

  const pick = () =>
    run(async () => {
      const path = await pickVaultFolder();
      if (path) onpick(path);
    });

  const inApp = () => run(async () => onpick(await deviceVault()));

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    e.stopPropagation();
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="phone-vault-title" tabindex="-1" {onkeydown}>
    <header>
      <h2 id="phone-vault-title">Vault 폴더 열기</h2>
      <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>
    <div class="body">
      <section>
        <h3>내 파일의 폴더</h3>
        <p class="note">
          <code>Documents</code> 같은 공용 폴더의 vault를 엽니다. Syncthing 같은 앱으로 PC의 vault와 맞출 수 있습니다.
        </p>
        {#if access && !access.granted}
          <p class="note warn">
            이 폴더들을 읽고 쓰려면 <b>모든 파일 접근</b> 권한이 필요합니다. 설정 화면에서 Rusidian을 허용한 뒤 돌아오세요.
          </p>
          <button class="primary" onclick={allow} disabled={busy}>권한 허용하러 가기</button>
        {:else}
          <button class="primary" onclick={pick} disabled={busy || !access}>폴더 고르기</button>
        {/if}
      </section>
      <section>
        <h3>앱 안의 vault</h3>
        <p class="note">권한 없이 쓸 수 있지만 다른 앱이나 PC와 맞출 수 없습니다.</p>
        <button onclick={inApp} disabled={busy}>앱 안의 vault 열기</button>
      </section>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .dialog {
    width: min(440px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 14px;
  }
  .close {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .body {
    padding: 4px 20px 16px;
    overflow: auto;
    min-height: 0;
  }
  section {
    padding: 12px 0;
  }
  section + section {
    border-top: 1px solid var(--border);
  }
  .note {
    margin: 0 0 10px;
    font-size: 13px;
    color: var(--text-muted);
  }
  .note.warn {
    color: var(--text);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  button:not(.close) {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 10px 14px;
    cursor: pointer;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    margin: 10px 0 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: 13px;
  }
</style>
