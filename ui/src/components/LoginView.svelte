<script lang="ts">
  /**
   * 登录闸：http 后端开启访问保护（AIYA_ADMIN_PASSWORD / AIYA_TOKEN）时的
   * 唯一入口视图。
   *
   * 触发链：transport 收到 401 → 派发 `aiya:gate` 事件 → App 切到此视图；
   * mount 前 /auth/state 预检直接落闸（首帧即登录卡，主界面不闪现）。
   * 登录成功后整页重载——引导/订阅/引擎以新会话重建，不做半套状态续用。
   * 背景与卡片沿用主应用的层（AmbientBackdrop 的固定渐变层 + k-dlg 的
   * token 口径），不另造一套视觉。
   *
   * 凭据形态由 /auth/state 的 mode 决定：password（login 端点换会话）或
   * token（只发静态令牌的部署没有 login 端点，令牌即凭据直接入保险库）。
   */
  import { onMount } from 'svelte';
  import AmbientBackdrop from './AmbientBackdrop.svelte';
  import { t } from '../lib/i18n.svelte';

  let password = $state('');
  let mode = $state<'password' | 'token'>('password');
  let err = $state('');
  let busy = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);

  onMount(async () => {
    try {
      const r = await fetch('/auth/state');
      const s = (await r.json()) as { mode?: string };
      if (s.mode === 'token') mode = 'token';
    } catch {
      /* 探测失败保持口令形态：login 401 错误文案会兜底 */
    }
    inputEl?.focus();
  });

  async function submit() {
    if (busy) return;
    busy = true;
    err = '';
    if (mode === 'token') {
      // 静态令牌部署：令牌即凭据（校验交给首次 API 调用），入保险库重载
      try {
        localStorage.setItem('aiya.authToken', password.trim());
      } catch {
        /* 隐私模式：仅本次会话内存持有 */
      }
      location.reload();
      return;
    }
    try {
      const r = await fetch('/auth/login', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ password }),
      });
      if (r.ok) {
        const { token } = (await r.json()) as { token: string };
        try {
          localStorage.setItem('aiya.authToken', token);
        } catch {
          /* 隐私模式：仅本次会话内存持有 */
        }
        // 带令牌整页重载：引导/订阅/引擎以新会话重建
        location.reload();
        return;
      }
      err = r.status === 401 ? t('gate.wrong') : t('gate.failed', { status: r.status });
    } catch (e) {
      err = t('gate.failed', { status: String(e) });
    }
    busy = false;
    inputEl?.focus();
  }
</script>

<div class="gate">
  <AmbientBackdrop source={null} enabled={false} shape="ridge" glow="soft" paused={true} />
  <section class="card" aria-label={t('gate.title')}>
    <!-- 品牌块与 TitleBar 的 .tb-brand 同款（mark + 字标 + 副标，token 一致） -->
    <h1 class="brand">
      <span class="mark">A</span>
      <strong>AIYA</strong>
      <span class="sub">MUSIC</span>
    </h1>
    <p>{mode === 'token' ? t('gate.tokenHint') : t('gate.hint')}</p>
    <input
      bind:this={inputEl}
      bind:value={password}
      type="password"
      autocomplete="current-password"
      onkeydown={(e) => e.key === 'Enter' && submit()}
    />
    <button onclick={submit} disabled={busy}>{t('gate.enter')}</button>
    {#if err}
      <p class="err" role="alert">{err}</p>
    {/if}
  </section>
</div>

<style>
  /* 门占满视口，AmbientBackdrop 的固定渐变层做背景（与主应用同源） */
  .gate {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* 卡片视觉与 Dialog 的 .k-dlg 同口径（token 一致即观感一致） */
  .card {
    position: relative;
    z-index: var(--z-content);
    width: 380px;
    max-width: calc(100vw - 48px);
    padding: 26px 28px;
    border: 1px solid var(--line-strong);
    border-radius: 14px;
    background: var(--surface-3);
    box-shadow: var(--shadow-5);
    backdrop-filter: blur(var(--blur-modal));
    animation: gate-pop 120ms var(--ease);
  }

  /* 品牌块：视觉复刻 .tb-brand（mark 方块 + 字标 + 副标），去定宽/侧栏对齐 */
  .brand {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 4px;
    letter-spacing: 0.14em;
  }

  .brand .mark {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    background: var(--accent);
    color: var(--surface-1);
    font-size: var(--fs-md);
    font-weight: 700;
    letter-spacing: 0;
  }

  .brand strong {
    font-size: var(--fs-lg);
    font-weight: 700;
    color: var(--text-1);
  }

  .brand .sub {
    font-size: var(--fs-xs);
    color: var(--text-2);
    letter-spacing: 0.22em;
  }

  .card p {
    margin: 0 0 16px;
    font-size: 12px;
    color: var(--text-3);
  }

  .card input {
    width: 100%;
    box-sizing: border-box;
    padding: 9px 12px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-1);
    color: var(--text-1);
    outline: none;
  }

  .card input:focus-visible {
    border-color: var(--accent);
  }

  .card button {
    width: 100%;
    margin-top: 12px;
    padding: 9px 12px;
    border: 0;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
    cursor: pointer;
  }

  .card button:disabled {
    opacity: 0.6;
  }

  .err {
    margin: 10px 0 0;
    min-height: 18px;
    font-size: 12px;
    color: var(--danger, #e06c75);
  }

  @keyframes gate-pop {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .card {
      animation: none;
    }
  }
</style>
