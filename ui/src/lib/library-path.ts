/** POSIX 根目录与 Windows 卷根保留末尾分隔符，其余路径去掉尾部分隔符。 */
export function normalizeRoot(value: string): string {
  const path = value.trim();
  if (/^\/+$/u.test(path)) return '/';
  if (/^[a-z]:[\\/]+$/iu.test(path)) return `${path.slice(0, 2)}${path[2]}`;
  return /^[a-z]:[\\/]|^\\\\/iu.test(path) ? path.replace(/[\\/]+$/u, '') : path.replace(/\/+$/u, '');
}

export function rootKey(value: string): string {
  const path = normalizeRoot(value);
  return /^[a-z]:[\\/]|^\\\\/iu.test(path) ? path.replaceAll('\\', '/').toLowerCase() : path;
}

export function absoluteRoot(value: string): boolean {
  return /^(?:\/|[a-z]:[\\/]|\\\\[^\\]+\\[^\\]+)/iu.test(value.trim());
}
