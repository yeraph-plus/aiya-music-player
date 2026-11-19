export interface DrawRegion {
  x: number; y: number; width: number; height: number;
  clipX: number; clipY: number; clipWidth: number; clipHeight: number;
}

/** 布局变化时测量区域；CSS 缩放统一换算为 canvas 的逻辑像素。 */
export class RegionTracker {
  private dirty = true;
  private readonly resize: ResizeObserver;
  private readonly mutation: MutationObserver;
  private readonly ancestors: HTMLElement[] = [];
  private readonly transitions = new Set<string>();
  private region: DrawRegion | null = null;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly target: HTMLElement) {
    this.resize = new ResizeObserver(this.invalidate);
    this.mutation = new MutationObserver(this.invalidate);
    for (let node: HTMLElement | null = target; node; node = node.parentElement) {
      this.ancestors.push(node);
      this.resize.observe(node);
      this.mutation.observe(node, { attributes: true, attributeFilter: ['style', 'class', 'hidden'] });
      node.addEventListener('transitionrun', this.transitionStart);
      node.addEventListener('transitionend', this.transitionEnd);
      node.addEventListener('transitioncancel', this.transitionEnd);
    }
    this.resize.observe(canvas);
    window.addEventListener('resize', this.invalidate);
    document.addEventListener('scroll', this.invalidate, true);
    window.visualViewport?.addEventListener('resize', this.invalidate);
    window.visualViewport?.addEventListener('scroll', this.invalidate);
  }

  private invalidate = (): void => { this.dirty = true; };
  private transitionStart = (event: TransitionEvent): void => {
    if (event.target !== event.currentTarget) return;
    this.transitions.add(`${this.ancestors.indexOf(event.currentTarget as HTMLElement)}:${event.propertyName}`);
    this.invalidate();
  };
  private transitionEnd = (event: TransitionEvent): void => {
    if (event.target !== event.currentTarget) return;
    this.transitions.delete(`${this.ancestors.indexOf(event.currentTarget as HTMLElement)}:${event.propertyName}`);
    this.invalidate();
  };

  measure(): DrawRegion | null {
    if (!this.dirty && this.transitions.size === 0) return this.region;
    this.dirty = false;
    const canvas = this.canvas.getBoundingClientRect(), target = this.target.getBoundingClientRect();
    if (!this.target.isConnected || !target.width || !target.height || !canvas.width || !canvas.height) {
      this.region = null;
      return null;
    }
    let left = Math.max(canvas.left, target.left), right = Math.min(canvas.right, target.right);
    let top = Math.max(canvas.top, target.top), bottom = Math.min(canvas.bottom, target.bottom);
    for (const ancestor of this.ancestors.slice(1)) {
      const style = getComputedStyle(ancestor);
      if (style.overflowX === 'visible' && style.overflowY === 'visible') continue;
      const rect = ancestor.getBoundingClientRect();
      if (style.overflowX !== 'visible') { left = Math.max(left, rect.left); right = Math.min(right, rect.right); }
      if (style.overflowY !== 'visible') { top = Math.max(top, rect.top); bottom = Math.min(bottom, rect.bottom); }
    }
    if (right <= left || bottom <= top) { this.region = null; return null; }
    const sx = this.canvas.clientWidth / canvas.width, sy = this.canvas.clientHeight / canvas.height;
    this.region = {
      x: (target.left - canvas.left) * sx, y: (canvas.bottom - target.bottom) * sy,
      width: target.width * sx, height: target.height * sy,
      clipX: (left - canvas.left) * sx, clipY: (canvas.bottom - bottom) * sy,
      clipWidth: (right - left) * sx, clipHeight: (bottom - top) * sy,
    };
    return this.region;
  }

  dispose(): void {
    this.resize.disconnect(); this.mutation.disconnect();
    for (const node of this.ancestors) {
      node.removeEventListener('transitionrun', this.transitionStart);
      node.removeEventListener('transitionend', this.transitionEnd);
      node.removeEventListener('transitioncancel', this.transitionEnd);
    }
    window.removeEventListener('resize', this.invalidate);
    document.removeEventListener('scroll', this.invalidate, true);
    window.visualViewport?.removeEventListener('resize', this.invalidate);
    window.visualViewport?.removeEventListener('scroll', this.invalidate);
    this.transitions.clear();
  }
}
