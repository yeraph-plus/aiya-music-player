import { t } from './i18n.svelte';

export type LoginMode = 'password' | 'token';

export interface LoginAuth {
  mode: () => Promise<LoginMode>;
  submit: (credential: string, mode: LoginMode) => Promise<void>;
}

function storeToken(token: string): void {
  try { localStorage.setItem('aiya.authToken', token); }
  catch { /* 隐私模式下不持久化凭据。 */ }
}

/** HTTP 登录闸的凭据交换与重载；展示层只消费模式与提交结果。 */
export const httpLogin: LoginAuth = {
  mode: async () => {
    const response = await fetch('/auth/state');
    const state = await response.json() as { mode?: string };
    return state.mode === 'token' ? 'token' : 'password';
  },
  submit: async (credential, mode) => {
    if (mode === 'token') {
      storeToken(credential.trim());
      location.reload();
      return;
    }
    let response: Response;
    let token = '';
    try {
      response = await fetch('/auth/login', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ password: credential }),
      });
      if (response.ok) token = ((await response.json()) as { token: string }).token;
    } catch (error) {
      throw new Error(t('gate.failed', { status: String(error) }));
    }
    if (!response.ok) throw new Error(response.status === 401 ? t('gate.wrong') : t('gate.failed', { status: response.status }));
    storeToken(token);
    location.reload();
  },
};
