import type { LoginAuth } from '../lib/login';
import { t } from '../lib/i18n.svelte';

/** 预览凭据只参与内存判定，不请求认证端点，也不写入认证存储。 */
export function previewAuth(params: URLSearchParams): LoginAuth {
  const mode = params.get('gate') === 'token' ? 'token' : 'password';
  return {
    mode: async () => mode,
    submit: async credential => {
      await new Promise(resolve => setTimeout(resolve, 250));
      if (credential !== 'preview') throw new Error(t('gate.wrong'));
      const url = new URL(location.href);
      url.searchParams.delete('gate');
      location.assign(url);
    },
  };
}
