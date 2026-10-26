<script lang="ts">
  import {
    FolderOpen,
    Music,
    Play,
    Plus,
    Search,
    Settings,
    Shuffle,
    Trash2,
  } from '@lucide/svelte';
  import Header from '../components/content/Header.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import Checkbox from '../components/ui/Checkbox.svelte';
  import Chip from '../components/ui/Chip.svelte';
  import Cover from '../components/ui/Cover.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import Field from '../components/ui/Field.svelte';
  import IconButton from '../components/ui/IconButton.svelte';
  import Progress from '../components/ui/Progress.svelte';
  import SegmentedControl from '../components/ui/SegmentedControl.svelte';
  import Skeleton from '../components/ui/Skeleton.svelte';
  import Slider from '../components/ui/Slider.svelte';
  import StatBlock from '../components/ui/StatBlock.svelte';
  import Switch from '../components/ui/Switch.svelte';

  /**
   * Kit 演示页（`?page=kit`，仅 dev）。
   *
   * 两个用途，缺一不可：
   * 1. **自测**：14 件的全状态在一屏里，改 token 后刷新这一页就能看出哪件没跟上。
   * 2. **约定**：给后续批次一个"正确用法"的参照 —— 比如 Checkbox 为什么外面套
   *    一层 `tabindex="-1"` 的 span（见组件头注释），抄这一页就不会抄错。
   *
   * 不注册进 `AppPage`：它不是产品页面，不该进 UI 状态持久化（否则用户关掉后
   * 下次启动会落在演示页）。`App.svelte` 里按 `import.meta.env.DEV` + 查询串分支。
   */

  let checkboxA = $state(false);
  let checkboxB = $state(true);
  let indeterminate = $state(true);
  let switchOn = $state(true);
  let density = $state<'comfortable' | 'compact'>('comfortable');
  let volume = $state(64);
  let position = $state(38);
  let committed = $state(38);
  let text = $state('');
  let tab = $state('all');
  let toolbarOn = $state(true);
  const fieldError = '这里显示校验不过的原因';

  const tabs = [
    { id: 'all', label: '全部曲目', count: 1284 },
    { id: 'recent', label: '最近播放' },
    { id: 'most', label: '常听', count: 12 },
  ];

  const demoCover =
    "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='92' height='92'%3E%3Crect width='92' height='92' fill='%235a6cff'/%3E%3Ctext x='46' y='58' font-size='30' text-anchor='middle' fill='white'%3E%E2%99%AA%3C/text%3E%3C/svg%3E";

  // snippet 是值：先定义再作为属性传（`icon={(<Play />)}` 是 React 写法，Svelte 没有）
</script>

{#snippet playIcon()}<Play size={13} />{/snippet}
{#snippet plusIcon()}<Plus size={13} />{/snippet}
{#snippet trashIcon()}<Trash2 size={13} />{/snippet}
{#snippet folderIcon()}<FolderOpen size={20} />{/snippet}
{#snippet musicIcon()}<Music size={26} />{/snippet}

<div class="page-scroll kit">
  <!-- Header 三态：纯文字 / 大封面 / 占位符 -->
  <Header
    kicker="UI KIT"
    title="零件演示"
    subtitle="B2 第一批 14 件 + Content 层两个版式件。改 token 后刷新这一页即可目检。"
    meta="14 件 · 全状态"
  />

  <Header
    kicker="播放列表"
    title="有封面的页头"
    subtitle="封面三态之一：给字符串就显示 92px 大图。"
    meta="42 首 · 2 小时 51 分"
    cover={demoCover}
  />

  <Header
    kicker="播放列表"
    title="无封面的页头"
    subtitle="封面三态之三：null = 有封面位但没图。"
    meta="0 首"
    cover={null}
  />

  <Toolbar
    {tabs}
    active={tab}
    ontab={(id) => (tab = id)}
    filters={toolbarOn ? [{ label: '2020 年后', onclear: () => (toolbarOn = false) }] : []}
  >
    {#snippet end()}
      <Button size="sm" variant="ghost" onclick={() => (toolbarOn = !toolbarOn)}>
        {toolbarOn ? '清掉筛选' : '恢复筛选'}
      </Button>
    {/snippet}
  </Toolbar>

  <h2>1 · Button</h2>
  <div class="row">
    <Button variant="primary" icon={playIcon}>播放全部</Button>
    <Button icon={plusIcon}>新建</Button>
    <Button variant="ghost">次要</Button>
    <Button variant="danger" icon={trashIcon}>删除</Button>
    <Button size="sm">小号</Button>
    <Button loading>保存中</Button>
    <Button disabled>不可用</Button>
  </div>

  <h2>2 · IconButton（label 必填）</h2>
  <div class="row">
    <IconButton label="上一首" size="sm">‹</IconButton>
    <IconButton label="播放"><Play size={15} /></IconButton>
    <IconButton label="设置" size="lg"><Settings size={18} /></IconButton>
    <IconButton label="随机播放" pressed><Shuffle size={15} /></IconButton>
    <IconButton label="当前项高亮" active><Music size={15} /></IconButton>
    <IconButton label="不可用" disabled><Search size={15} /></IconButton>
  </div>

  <h2>3 · Chip</h2>
  <div class="row">
    <Chip label="静态" />
    <Chip label="选中" on />
    <Chip label="可点" onclick={() => (toolbarOn = !toolbarOn)} />
    <Chip label="可关" on onclear={() => (toolbarOn = false)} />
  </div>

  <h2>4 · Checkbox（外层包装层才有连选 / 行语义）</h2>
  <div class="row">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="hit" tabindex="-1" onclick={() => (checkboxA = !checkboxA)}>
      <Checkbox checked={checkboxA} label="未选" /> 未选
    </span>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="hit" tabindex="-1" onclick={() => (checkboxB = !checkboxB)}>
      <Checkbox checked={checkboxB} label="已选" /> 已选
    </span>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="hit" tabindex="-1" onclick={() => (indeterminate = !indeterminate)}>
      <Checkbox checked={false} indeterminate={indeterminate} label="半选" /> 半选
    </span>
    <span class="hit"><Checkbox checked={false} disabled label="禁用" /> 禁用</span>
  </div>

  <h2>5 · Switch</h2>
  <div class="row">
    <Switch checked={switchOn} onchange={(v) => (switchOn = v)} label="封面主色" />
    <span class="muted">{switchOn ? '开' : '关'}</span>
    <Switch checked={false} onchange={() => {}} label="禁用开关" disabled />
  </div>

  <h2>6 · SegmentedControl</h2>
  <div class="row">
    <SegmentedControl
      value={density}
      options={[
        { value: 'comfortable', label: '标准' },
        { value: 'compact', label: '紧凑' },
      ]}
      onchange={(v) => (density = v as typeof density)}
      label="列表密度"
    />
    <SegmentedControl
      value={density}
      size="sm"
      options={[
        { value: 'comfortable', label: '共享' },
        { value: 'compact', label: '独占' },
      ]}
      onchange={(v) => (density = v as typeof density)}
      label="输出模式（紧凑档）"
    />
  </div>

  <h2>7 · Slider（oninput 预览 / oncommit 提交）</h2>
  <div class="stack">
    <div class="row">
      <span class="cap">音量 {volume}</span>
      <Slider value={volume} min={0} max={100} label="音量" width="220px" oninput={(v) => (volume = v)} />
    </div>
    <div class="row">
      <span class="cap">拖动 {position} s / 已提交 {committed} s</span>
      <Slider
        value={position}
        min={0}
        max={240}
        label="播放进度"
        width="220px"
        oninput={(v) => (position = v)}
        oncommit={(v) => (committed = v)}
      />
    </div>
  </div>

  <h2>8 · Progress</h2>
  <div class="stack">
    <Progress value={62} label="扫描进度" />
    <Progress value={62} label="扫描进度（粗）" size="md" />
    <Progress value={null} label="不确定进度" />
  </div>

  <h2>9 · Field</h2>
  <div class="grid2">
    <Field label="搜索" placeholder="搜索本地曲库" type="search" value={text} oninput={(v) => (text = v)} hideLabel />
    <Field label="歌单名称" value={text} oninput={(v) => (text = v)} hint="会显示在侧栏，可随时改名" />
    <Field label="带错误" value="" oninput={() => {}} error={fieldError} />
    <Field label="路径（等宽）" value="D:/Music/Albums" mono oninput={() => {}} />
  </div>

  <h2>10 · Card / 11 · Cover</h2>
  <div class="grid3">
    <Card title="带封面" subtitle="42 首" cover={demoCover} />
    <Card title="无封面占位" subtitle="0 首" cover={null} />
    <Card title="图标卡" subtitle="曲库入口" icon={folderIcon} />
  </div>
  <div class="row covers">
    <Cover size="xs" />
    <Cover size="sm" />
    <Cover size="md" />
    <Cover size="lg" />
  </div>

  <h2>12 · Skeleton / 13 · EmptyState / 14 · StatBlock</h2>
  <div class="grid2">
    <div class="stack">
      <Skeleton kind="row" count={3} />
      <Skeleton kind="block" height="56px" />
    </div>
    <EmptyState title="还没有曲目" hint="扫描本地文件夹后，歌曲会出现在这里。" icon={musicIcon}>
      {#snippet action()}
        <Button variant="primary" size="sm">扫描曲库</Button>
      {/snippet}
    </EmptyState>
  </div>
  <div class="row stats">
    <StatBlock value={1284} label="曲目" />
    <StatBlock value="2 小时 51 分" label="总时长" size="sm" />
    <StatBlock value={3} label="缺失文件" size="lg" tone="warn" hint="下次扫描会重试" />
  </div>
</div>

<style>
  .kit {
    /* 演示页只做排版，不引入任何新的设计值 */
    max-width: 980px;
  }

  h2 {
    margin: 26px 0 10px;
    font-size: var(--fs-sm);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 420px;
  }

  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
  }

  .grid3 {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-3);
  }

  .hit {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-base);
    color: var(--text-1);
    cursor: pointer;
  }

  .cap,
  .muted {
    font-size: var(--fs-sm);
    color: var(--text-2);
    min-width: 150px;
    font-variant-numeric: tabular-nums;
  }

  .covers {
    margin-top: var(--space-3);
    align-items: flex-end;
  }

  .stats {
    gap: var(--space-6);
    margin: var(--space-4) 0 30px;
  }
</style>
