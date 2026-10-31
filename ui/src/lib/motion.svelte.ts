/**
 * 响应式的 `prefers-reduced-motion`（发布前 UI 审查 P2-8）。
 *
 * 此前三个组件（SideDrawer / NowPlayingSpectrum / AmbientBackdrop）各自在
 * 创建时 `matchMedia(...).matches` 读**一次**——运行中改系统设置不生效，
 * 得重启应用。这里收敛为单一信号源：模块级 `$state` + change 监听。
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
