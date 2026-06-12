/**
 * Re-exports públicos del módulo `api`.
 *
 * `import { fetchLogs, type LogRow } from '$lib/api'` sigue funcionando: cada
 * fetch/tipo vive en un archivo por recurso (`logs.ts`, `traces.ts`, …) y este
 * `index.ts` solo los re-exporta. El wrapper `api<T>`, `apiBase`, `qs` y el
 * tipo `RangeArgs` viven en `core.ts` (importados y re-exportados acá).
 *
 * Contrato: agregar un export acá Y en el archivo de recurso. El test
 * `api.test.ts` valida que `apiBase` y `parseCohortDefinition` siguen
 * accesibles.
 */
export { api, apiBase, qs, UnauthorizedError, type RangeArgs } from './core';

export { fetchDashboard, type Dashboard } from './dashboard';
export { fetchServices, fetchServiceMap, type Service, type ServiceMap, type ServiceMapNode, type ServiceMapEdge } from './services';
export { fetchLogs, fetchLogStats, type LogRow } from './logs';
export { fetchTraces, fetchTrace, type TraceSummary, type SpanRow } from './traces';
export { fetchMetricNames, fetchMetricSeries, type MetricName, type Point } from './metrics';
export { fetchIssues, fetchIssue, updateIssueStatus, fetchIssueSessions, type Issue, type ErrorEvent, type IssueSession } from './issues';
export {
  fetchMonitors, fetchMonitorResults, fetchMonitorUptime, createMonitor, updateMonitor, deleteMonitor,
  type Monitor, type MonitorResult
} from './monitors';
export {
  fetchAlertRules, fetchAlertIncidents, createAlertRule, updateAlertRule, deleteAlertRule,
  type AlertRule, type AlertIncident
} from './alerts';
export { fetchReplays, fetchReplay, type ReplaySummary, type ReplayPayload } from './replays';
export { fetchEvents, fetchEventStats, type ProductEvent, type EventBucket, type EventFilters } from './productEvents';
export { fetchRetention, type RetentionCohort, type RetentionResult, type RetentionFilters } from './retention';
export {
  fetchProductSessions, fetchProductSessionTraces,
  type ProductSessionSummary, type ProductSessionFilters
} from './productSessions';
export {
  fetchProductUsers, fetchProductUser, fetchProductUserEvents,
  type ProductUserSummary, type ProductUserDetail, type ProductUserDeviceBreakdown, type ProductUserFilters
} from './productUsers';
export {
  fetchFunnelEvents, computeFunnel, previewDropOff, previewTimeToConvert,
  type EventCandidate, type FunnelStep, type FunnelResult, type FunnelRequest,
  type DropOffEvent, type DropOffResult, type DropOffRequest,
  type TimeBin, type TimeToConvertResult, type TimeToConvertRequest
} from './funnels';
export {
  analyzeExperiment,
  type ExperimentVariantResult, type ExperimentAnalyzeRequest, type ExperimentAnalyzeResult
} from './experiments';
export {
  fetchServiceDashboardInsight,
  type ServiceDashboardIssue, type ServiceDashboardInsight, type ServiceDashboardFilters
} from './insights';
export {
  parseCohortDefinition,
  listCohorts, getCohort, createCohort, updateCohort, deleteCohort,
  previewCohort, fetchCohortUsers, fetchCohortRetention, fetchCohortOverlap,
  type CohortFilter, type CohortDefinition, type Cohort, type CohortInput,
  type CohortPreview, type CohortRetentionPoint, type CohortRetention, type CohortOverlap
} from './cohorts';
export {
  fetchProjects, fetchProject, createProject, updateProject, deleteProject, rotateProjectToken,
  type Project
} from './projects';
export {
  fetchRedaction, saveRedaction,
  type RedactionCustomRule, type RedactionConfig, type RedactionBuiltinInfo, type RedactionView
} from './redaction';
export { fetchOrigins, saveOrigins, type OriginConfig, type OriginsView } from './origins';
export {
  fetchTelegramIntegration, saveTelegramIntegration, deleteTelegramIntegration, testTelegramIntegration,
  type TelegramIntegration, type TelegramInput
} from './integrations';
export {
  fetchChannelKinds, listChannels, getChannel, createChannel, updateChannel, deleteChannel, testChannel,
  type ChannelKind, type NotificationChannel, type ChannelInput
} from './channels';
export {
  login, loginTotp, logout, me,
  fetchSessions, revokeOtherSessions,
  fetchTwoFaStatus, twoFaSetup, twoFaEnable, twoFaDisable, twoFaRegenRecovery,
  type AuthUser, type LoginResponse, type SessionInfo,
  type TwoFaStatus, type TwoFaSetup, type TwoFaEnableResult
} from './auth';
export {
  fetchUsers, createUser, updateUser, deleteUser, changeUserPassword, type User
} from './users';
export {
  fetchPreferences, savePreferences,
  type ThemePref, type TimeRangePref, type Preferences, type PreferencesPatch
} from './preferences';
