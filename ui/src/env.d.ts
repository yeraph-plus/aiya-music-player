import type { AiyaApi } from '@aiya/contracts';

declare global {
  interface Window {
    api: AiyaApi;
  }
}

export {};
