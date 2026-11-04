/**
 * i18n 基建（docs/64 §3.2）：平面键值表 + 响应式 locale。
 *
 * 语言值来自应用设置 `language`（首启由宿主按系统 UI 语言写库，之后恒按
 * 设置值）——App.svelte 的 effect 调 setLocale 跟随，并把 `<html data-lang>`
 * 一并设置（装饰字取消等纯 CSS 分支靠它）。
 *
 * 域表在 `i18n/domains/*.ts`：每个迁移分片一个域段（zh/en 成对，en 用
 * `Record<keyof typeof zh, string>` 保证键位奇偶），在此登记合并——
 * 键类型从合并后的 zh 推导，任何一域缺键都会编译报错。
 */

import { coreZh, coreEn } from './i18n/domains/core';
import { settingsZh, settingsEn } from './i18n/domains/settings';
import { shellZh, shellEn } from './i18n/domains/shell';
import { libraryZh, libraryEn } from './i18n/domains/library';
import { liveZh, liveEn } from './i18n/domains/live';
import { beetsZh, beetsEn } from './i18n/domains/beets';
import { libZh, libEn } from './i18n/domains/lib';
import { nowZh, nowEn } from './i18n/domains/now';
import { eqZh, eqEn } from './i18n/domains/eq';

const zh = { ...coreZh, ...settingsZh, ...shellZh, ...libraryZh, ...liveZh, ...beetsZh, ...libZh, ...nowZh, ...eqZh };
const en: Record<keyof typeof zh, string> = {
  ...coreEn,
  ...settingsEn,
  ...shellEn,
  ...libraryEn,
  ...liveEn,
  ...beetsEn,
  ...libEn,
  ...nowEn,
  ...eqEn,
};

export type Msg = keyof typeof zh;
export type Locale = 'zh' | 'en';

let locale = $state<Locale>('zh');

export function setLocale(l: Locale): void {
  locale = l;
}

export function currentLocale(): Locale {
  return locale;
}

/** 取文案；`{name}` 形态占位符用 params 替换。未知键回退 zh 表原键名。 */
export function t(key: Msg, params?: Record<string, string | number>): string {
  const table = locale === 'en' ? en : zh;
  let s = table[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, String(v));
  }
  return s;
}
