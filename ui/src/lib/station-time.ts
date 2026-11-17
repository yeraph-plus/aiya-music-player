import type { StationQueueItem, StationSlot } from './ipc/generated';

export const LIBRARY_SOURCE = '__library__';
export const WEEK_DAYS = ['station.schMon','station.schTue','station.schWed','station.schThu','station.schFri','station.schSat','station.schSun'] as const;
export const hm = (minutes: number): string => `${String(Math.floor(minutes / 60)).padStart(2,'0')}:${String(minutes % 60).padStart(2,'0')}`;
export const minutes = (value: string): number => Number(value.slice(0,2)) * 60 + Number(value.slice(3));

/** 星期掩码描述开始日；次日与周日跨周均在周分钟坐标上取模。 */
export function covers(slot: StationSlot, day: number, from: number, until = from + 1): boolean {
  if(!validSlot(slot))return false;
  const start = minutes(slot.startHm), end = minutes(slot.endHm);
  const length = end > start ? end - start : 1440 - start + end;
  for (let d=0;d<7;d++) {
    if (!(slot.daysMask & (1<<d))) continue;
    const base = d*1440+start;
    for (const origin of [day*1440,day*1440+10080]) {
      if (origin+from < base+length && origin+until > base) return true;
    }
  }
  return false;
}

export function validSlot(slot: StationSlot): boolean {
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(slot.startHm)
    && (/^([01]\d|2[0-3]):[0-5]\d$/.test(slot.endHm) || slot.endHm==='24:00')
    && slot.startHm!==slot.endHm && slot.daysMask>0 && slot.daysMask<=127;
}

export function conflicts(items: StationQueueItem[], candidate: StationSlot, owner: string): boolean {
  const priority=items.find(i=>i.playlistId===owner)?.priority;
  return items.some(item => item.scheduled && item.playlistId!==owner && item.priority===priority && item.slots.some(slot => {
    if(!validSlot(slot) || !validSlot(candidate))return false;
    for (let day=0;day<7;day++) for (let minute=0;minute<1440;minute++) {
      if (covers(candidate,day,minute) && covers(slot,day,minute)) return true;
    }
    return false;
  }));
}

export function queueItemError(item: StationQueueItem, items: StationQueueItem[]): 'station.timeInvalid' | 'station.timeConflict' | 'station.priorityInvalid' | null {
  if(!Number.isInteger(item.priority) || item.priority<0 || item.priority>65535)return 'station.priorityInvalid';
  if(item.slots.some(s=>!validSlot(s)) || item.scheduled && !item.slots.length)return 'station.timeInvalid';
  if(item.scheduled && item.slots.some(s=>conflicts(items,s,item.playlistId)))return 'station.timeConflict';
  return null;
}

export function programError(items: StationQueueItem[]): ReturnType<typeof queueItemError> {
  return items.map(item=>queueItemError(item,items)).find(Boolean) ?? null;
}
