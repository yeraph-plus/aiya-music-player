import type { AlbumFacet, ArtistFacet } from '../lib/ipc';
import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';

/**
 * 专辑 / 艺人聚合数据（宫格浏览的按需视图）。
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
  /** 过滤后总数（后端截断到 500 不静默：total > items.length 即有更多） */
  albumsTotal = $state(0);
  artistsTotal = $state(0);
  albumsLoading = $state(false);
  artistsLoading = $state(false);
  albumsQuery = $state<string | null>(null);
  artistsQuery = $state<string | null>(null);
  albumsError = $state('');
  artistsError = $state('');
  loading = $derived(this.albumsLoading || this.artistsLoading);

  /**
   * 请求代次，按字段各持一个（与 stores/list 的 reload 同一模式）。
   * 不共用一个计数的原因：搜索页同一轮会**并发**调 loadAlbums 与
   * loadArtists，共享代次会让先发出的那个响应被误判过期丢弃。
   */
  private albumsGeneration = 0;
  private artistsGeneration = 0;

  /**
   * 拉专辑宫格。空白 search = 全量（前 500，total 可大于 items）。
   * 请求状态与结果查询词一起保存，页面据此区分加载、失败与无匹配结果。
   */
  async loadAlbums(search?: string): Promise<AlbumFacet[]> {
    const generation = (this.albumsGeneration += 1);
    const query = search?.trim() ?? '';
    this.albumsLoading = true;
    this.albumsError = '';
    try {
      const page = await ipc.library.albums(query || undefined);
      if (generation === this.albumsGeneration) {
        this.albums = page.items;
        this.albumsTotal = page.total;
        this.albumsQuery = query;
      }
      return page.items;
    } catch (err) {
      if (generation === this.albumsGeneration) {
        this.albumsError = errorMessage(err);
        this.albumsQuery = query;
        this.albums = [];
        this.albumsTotal = 0;
      }
      return [];
    } finally {
      // 过期响应不碰 loading：新一轮还在飞行中，提前清掉会闪一帧空态
      if (generation === this.albumsGeneration) this.albumsLoading = false;
    }
  }

  async loadArtists(search?: string): Promise<ArtistFacet[]> {
    const generation = (this.artistsGeneration += 1);
    const query = search?.trim() ?? '';
    this.artistsLoading = true;
    this.artistsError = '';
    try {
      const page = await ipc.library.artists(query || undefined);
      if (generation === this.artistsGeneration) {
        this.artists = page.items;
        this.artistsTotal = page.total;
        this.artistsQuery = query;
      }
      return page.items;
    } catch (err) {
      if (generation === this.artistsGeneration) {
        this.artistsError = errorMessage(err);
        this.artistsQuery = query;
        this.artists = [];
        this.artistsTotal = 0;
      }
      return [];
    } finally {
      if (generation === this.artistsGeneration) this.artistsLoading = false;
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
