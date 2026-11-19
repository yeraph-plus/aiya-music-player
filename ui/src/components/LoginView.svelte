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
  import { onMount, tick } from 'svelte';
  import AmbientBackdrop from './AmbientBackdrop.svelte';
  import { provideRenderHost } from '../lib/render/host.svelte';
  import Field from './ui/Field.svelte';
  import Button from './ui/Button.svelte';
  import InlineError from './ui/InlineError.svelte';
  import { t } from '../lib/i18n.svelte';
  import { httpLogin, type LoginAuth, type LoginMode } from '../lib/login';

  let { auth = httpLogin }: { auth?: LoginAuth } = $props();
  const renderHost = provideRenderHost();

  let password = $state('');
  let mode = $state<LoginMode>('password');
  let err = $state('');
  let busy = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);

  onMount(async () => {
    try {
      mode = await auth.mode();
    } catch {
      /* 探测失败保持口令形态：login 401 错误文案会兜底 */
    }
    inputEl?.focus();
  });

  async function submit() {
    if (busy) return;
    busy = true;
    err = '';
    try {
      await auth.submit(password, mode);
    } catch (e) {
      err = e instanceof Error ? e.message : t('gate.failed', { status: String(e) });
    }
    busy = false;
    await tick();
    inputEl?.focus();
  }
</script>

<div class="gate">
  <AmbientBackdrop host={renderHost} source={null} enabled={false} shape="ridge" glow="soft" paused={true} />
  <section class="card" aria-label={t('gate.title')}>
    <!-- 品牌块与 TitleBar 的 .tb-brand 同款（mark + 字标 + 副标，token 一致） -->
    <h1 class="brand">
      <span class="mark">A</span>
      <strong>AIYA</strong>
      <span class="sub">MUSIC</span>
    </h1>
    <p>{mode === 'token' ? t('gate.tokenHint') : t('gate.hint')}</p>
    <form onsubmit={event => {event.preventDefault();void submit();}}>
      <Field label={mode === 'token' ? t('gate.token') : t('gate.password')} type="password"
        name={mode === 'token' ? 'token' : 'password'} autocomplete="current-password"
        bind:ref={inputEl} value={password} oninput={value => password = value} required disabled={busy}
        aria-invalid={!!err} aria-describedby={err ? 'gate-error' : undefined} />
      <Button variant="primary" type="submit" loading={busy}>{t('gate.enter')}</Button>
      {#if err}<InlineError id="gate-error" message={err} />{/if}
    </form>
  </section>
</div>

<style>
  /* 门占满视口，AmbientBackdrop 的固定渐变层做背景（与主应用同源） */
  .gate {
    position: fixed;
    inset: 0;
    padding:var(--space-5);
    overflow:auto;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* 卡片视觉与 Dialog 的 .k-dlg 同口径（token 一致即观感一致） */
  .card {
    position: relative;
    z-index: var(--z-content);
    width: 23.75rem;
    max-width: calc(100vw - 2 * var(--space-5));
    padding: var(--space-6);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-xl);
    background: var(--surface-4);
    box-shadow: var(--shadow-5);
    backdrop-filter: blur(var(--blur-modal));
    animation: gate-pop 120ms var(--ease);
  }

  /* 品牌块：视觉复刻 .tb-brand（mark 方块 + 字标 + 副标），去定宽/侧栏对齐 */
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-1);
    letter-spacing: 0.14em;
  }

  .brand .mark {
    display: inline-grid;
    place-items: center;
    width: var(--space-6);
    height: var(--space-6);
    border-radius: var(--radius-sm);
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
    margin: var(--space-4) 0 var(--space-5);
    font-size: var(--fs-md);
    line-height: 1.5;
    color: var(--text-2);
  }

  form {display:flex;flex-direction:column;gap:var(--space-4);}

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
