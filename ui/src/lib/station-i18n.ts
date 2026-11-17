import { t } from './i18n.svelte';
import type { StationProgramIssue, StationQueueSource } from './ipc/generated';
import { LIBRARY_SOURCE, WEEK_DAYS } from './station-time';

/** 节目状态使用结构化原因与来源标识，名称和星期在当前界面语言中呈现。 */
export function programIssueText(issue: StationProgramIssue, sources: StationQueueSource[]): string {
  const name = (id: string): string => id === LIBRARY_SOURCE
    ? t('station.allLibrary')
    : sources.find(source => source.playlistId === id)?.name ?? id;
  switch (issue.kind) {
    case 'emptyQueue': return t('station.issueEmptyQueue');
    case 'missingSource': return t('station.issueMissingSource', { name: name(issue.playlistId) });
    case 'emptySource': return t('station.issueEmptySource', { name: name(issue.playlistId) });
    case 'uncoveredTime': return t('station.issueUncoveredTime', {
      day: t('station.weekday', { day: t(WEEK_DAYS[issue.weekday]) }), time: issue.time,
    });
  }
}
