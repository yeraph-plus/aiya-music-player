import type { AlbumFacet, ArtistFacet } from '../lib/ipc';
import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { notify } from './notify.svelte';

/**
 * 专辑 / 艺人聚合数据（B4-3b）。
 *
 * 为什么独立 store 而不是塞进 `library`：`library` 持有的是"扫描 + 统计 +
 * 歌单摘要"这类**整库事实**，宫格数据是**按需拉取的浏览视图**——页面打开
 * 才调命令，扫描/移除后失效。职责不同，合并只会让 library 的加载时机表
 * 越来越长。
 *
 * 缓存策略：**内存级，页面级生命周期**。`load*` 每次都真调命令（聚合在
 * 5 万行量级是毫秒级，不值得为此做失效协议）；`albums`/`artists` 字段只
 * 是"最近一次结果"的镜像，供详情页头部查单条（`albumById`/`artistById`）。
 * 没有跨页缓存意味着详情页头部永远和列表页一致——一致性比省一次查询值钱。
 */
class FacetsStore {
  albums = $state<AlbumFacet[] | null>(null);
  artists = $state<ArtistFacet[] | null>(null);
  loading = $state(false);

  /**
   * 拉专辑宫格。空白 search = 全量（前 500）。
   * 失败走全局错误条（宫格页没有 InlineError 位），返回 [] 让页面渲染空态。
   */
  async loadAlbums(search?: string): Promise<AlbumFacet[]> {
    this.loading = true;
    try {
      const rows = await ipc.library.albums(search);
      this.albums = rows;
      return rows;
    } catch (err) {
      notify.showError(errorMessage(err));
      this.albums = [];
      return [];
    } finally {
      this.loading = false;
    }
  }

  async loadArtists(search?: string): Promise<ArtistFacet[]> {
    this.loading = true;
    try {
      const rows = await ipc.library.artists(search);
      this.artists = rows;
      return rows;
    } catch (err) {
      notify.showError(errorMessage(err));
      this.artists = [];
      return [];
    } finally {
      this.loading = false;
    }
  }

  /** 详情页头部查单条（在最近一次列表里找；找不到返回 null，页面自己兜底） */
  albumById(id: string): AlbumFacet | null {
    return this.albums?.find((a) => a.id === id) ?? null;
  }

  artistById(id: string): ArtistFacet | null {
    return this.artists?.find((a) => a.id === id) ?? null;
  }
}

export const facets = new FacetsStore();
