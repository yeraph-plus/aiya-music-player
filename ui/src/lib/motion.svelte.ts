/**
 * 响应式的 `prefers-reduced-motion`：单一信号源（模块级 `$state` + change 监听）。
 *
 * 各组件（SideDrawer / NowPlayingSpectrum / AmbientBackdrop 等）读这里的值，
 * 而不是各自 `matchMedia(...).matches` 只读**一次**——那样运行中改系统设置
 * 不生效，得重启应用。
 *
 * 用 `{ value }` 包一层而不是导出裸 let：Svelte 不允许导出会被重新赋值的
 * $state（state_invalid_export），只允许变更其属性。读法 `reducedMotion.value`。
 */
const query =
  typeof matchMedia !== 'undefined' ? matchMedia('(prefers-reduced-motion: reduce)') : null;

export const reducedMotion = $state({ value: query?.matches ?? false });

if (query) {
  query.addEventListener('change', (event) => {
    reducedMotion.value = event.matches;
  });
}
