<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte'
  import { projectClient, type CanvasEdge, type BaselineDraft, type BaselineVerificationJob, type DraftSummary, type ScanJob, type ScanReport, type CanvasNode, type ProjectClient, type ProjectSelection, type ProjectView } from './lib/api'
  import { isTauri } from '@tauri-apps/api/core'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import type { ReviewAction, ProjectState, PublicationPreview, PublicationApproval, PublicationReceipt } from './lib/api'
  import { createSample, sampleView, SAMPLE_NODE_IDS, transition as applySampleAction, type SampleState, type SampleAction } from './lib/sample'
  import SampleDetails from './components/SampleDetails.svelte'
  import DraftEditor from './components/DraftEditor.svelte'
  import DraftComparisonPanel from './components/DraftComparison.svelte'
  import type { DraftComparison } from './lib/api'
  import ProjectCanvas from './components/ProjectCanvas.svelte'
  import VerificationDetails from './components/VerificationDetails.svelte'
  import BaselineDetails from './components/BaselineDetails.svelte'
  import TaskDetails from './components/TaskDetails.svelte'
  import { moduleActivity, promptState, promptSummary, runtimeProjection, type RuntimeAttention } from './lib/runtime'
  import AttentionList from './components/AttentionList.svelte'
  import VisitDigest from './components/VisitDigest.svelte'
  import { inFocus, interruptionKey, muteUntil, newInterruptions, notification, signalEntries, visitDigest } from './lib/attention'
  import { lastVisit, recordVisit } from './lib/visit'
  import TaskCreate from './components/TaskCreate.svelte'
  import ProjectControl from './components/ProjectControl.svelte'
  import HumanMessage from './components/HumanMessage.svelte'
  import MainResolution from './components/MainResolution.svelte'
  import Proposals from './components/Proposals.svelte'
  import TrustLedger from './components/TrustLedger.svelte'
  import IdeaInput from './components/IdeaInput.svelte'
  import IdeaCards from './components/IdeaCards.svelte'
  import FrameTray from './components/FrameTray.svelte'
  import { blankSpot, defaultSize, frameBadges, frameChange, framesFrom, newFrameId, samePlace, TEMPLATE_MIME, withPins, type FrameActions, type FrameChoice } from './lib/frames'
  import { conversion, initialLayout, needsInitialLayout } from './lib/initial-layout'
  import AskPanel from './components/AskPanel.svelte'
  import FollowUps from './components/FollowUps.svelte'
  import { rhythmMarks } from './lib/follow-ups'
  import { followUpHint, ideasFrom } from './lib/ideas'
  import { actionObject, ledgerAction, receiptObject, type DelegationAction, type DelegationControls, type DelegationIntent, type RuleOffer } from './lib/delegation'
  import { activeTempRules, type AnswerDestination, type QuestionCard } from './lib/human'
  import { parseQuickCommand, QUICK_COMMANDS } from './lib/quick-commands'
  import { humanRequestId } from './lib/request-id'
  import type { AnswerRequest, PromptRequest, PromptScope, HumanReceipt, RunDetails, HumanTaskDraft, HumanTaskReceipt, ControlReceipt, ProjectControlRequest, ProjectControlState, DelegationReceipt, IdeaEntry, IdeaPromotion, IdeaReceipt, IdeaRequest, FollowUpRequest, FollowUpWhen, FollowUpEntry, FramePlace, FrameReceipt, FrameRequest, FrameTemplate, PlaceFrameRequest, CardRequest, FrameRulesRequest, FrameDecisionRequest, CardReceipt, FrameEntry, PinCardsRequest, TempRuleEntry, TempRuleRequest } from './lib/api'
  import { baselineReport, baselineEvidenceError, moduleStatuses, updateCandidate, type CardStatus } from './lib/verification'

  let { client = projectClient }: { client?: ProjectClient } = $props()
  let sample = $state.raw<SampleState | null>(null)
  let sampleError = $state<string | null>(null)
  let project = $state<ProjectSelection | null>(null)
  let failedSelection = $state<ProjectSelection | null>(null)
  let view = $state<ProjectView | null>(null)
  let error = $state<string | null>(null)
  let busy = $state(false)
  let selected = $state<CanvasNode | null>(null)
  let showProject = $state(false)
  let menu = $state<{ x: number; y: number } | null>(null)
  let fitVersion = $state(0)
  let generation = 0
  let selectionToken: string | null = null
  let scanGeneration = 0
  let scanTimer: ReturnType<typeof setTimeout> | undefined
  let scanning = $state(false)
  let activeJob = $state<ScanJob | null>(null)
  let startPending = $state(false)
  let cancelling = $state(false)
  let showExecution = $state(false)
  let jobActive = $derived(activeJob?.status === 'running')
  let scanError = $state<string | null>(null)
  let report = $state<ScanReport | null>(null)
  let candidate = $state(false)
  let draft = $state<BaselineDraft | null>(null)
  let drafts = $state<DraftSummary[]>([])
  let showDrafts = $state(false)
  let showDraft = $state(false)
  let draftLoading = $state(false)
  let draftError = $state<string | null>(null)
  let draftGeneration = 0
  let editing = $state(false)
  let showReference = $state(false)
  let referencePath = $state('')
  let referencePoint = $state({ x: 0, y: 0 })
  let editBusy = $state(false)
  let editError = $state<string | null>(null)
  let pendingEdit = $state<{ token: string; id: string; action: ReviewAction } | null>(null)
  let verificationJob = $state<BaselineVerificationJob | null>(null)
  let verificationBusy = $state(false)
  let verificationError = $state<string | null>(null)
  let pendingVerification = $state<{ token: string; parent: string; id: string } | null>(null)
  let verificationTimer: ReturnType<typeof setTimeout> | undefined
  let publication = $state<{ token: string; preview: PublicationPreview } | null>(null)
  let publicationBusy = $state(false)
  let publicationAttempted = $state(false)
  let publicationError = $state<string | null>(null)
  let published = $state<PublicationReceipt | null>(null)
  let projectState = $state<ProjectState | null>(null)
  let stateLoading = $state(false)
  let stateError = $state<string | null>(null)
  let liveGeneration = 0
  let liveSubscription: string | null = null
  let liveReadGeneration = 0
  let minimumSeq = $state(0)
  let connectionError = $state<string | null>(null)
  let liveInFlight = false
  let showTasks = $state(false)
  let showModuleTasks = $state(false)
  let runDetails = $state<Record<string, RunDetails>>({})
  let detailsLoading = $state<string | null>(null)
  let detailsError = $state<string | undefined>(undefined)
  let detailsGeneration = 0
  const taskPositions = new Map<string, { x: number; y: number }>()
  let runtime = $derived(runtimeProjection(projectState))
  /** Since the person last closed this project (FR-42); local to this machine. */
  let visit = $state<{ since: number; fallback: boolean } | null>(null)
  let digestOpen = $state(false)
  let lastDelegationKind = $state<DelegationAction['kind'] | null>(null)
  let digest = $derived(visit && projectState?.status === 'ready' ? visitDigest({ ...visit, now: Date.now(), tasks: runtime.tasks,
    delegation: runtime.delegation, signals: (projectState.projection as { TowerApi?: unknown } | undefined)?.TowerApi,
    attention: runtime.allAttention, budget: runtime.shown }) : null)
  let activity = $derived(!sample && !candidate && !draft
    ? moduleActivity(runtime.tasks, runtime.allAttention, (view?.nodes ?? []).filter((node) => node.object_type === 'Module').map((node) => node.object_id || node.id))
    : {})
  let moduleTasks = $derived(selected ? runtime.tasks.filter((task) => task.input.module === (selected?.object_id || selected?.id)) : [])
  let moduleAttention = $derived(selected ? runtime.allAttention.filter((item) => !item.modules.length || item.modules.includes(selected?.object_id || selected?.id || '')) : [])
  let recovery = $state<{ token: string; id: string } | null>(null)
  let recoveryBusy = $state(false)
  let recoveryAttempted = $state(false)
  let recoveryError = $state<string | null>(null)
  let recoveryCommit = $state<string | null>(null)
  type HumanContext = { kind: 'answer'; question: QuestionCard } | { kind: 'prompt'; scope: PromptScope; initialText?: string }
  type HumanRequest = { token: string; context: HumanContext } & ({ kind: 'answer'; input: AnswerRequest } | { kind: 'prompt'; input: PromptRequest }
    | { kind: 'rule'; input: TempRuleRequest })
  let humanContext = $state<HumanContext | null>(null)
  let showHuman = $state(false)
  let pendingHuman = $state<HumanRequest | null>(null)
  let humanBusy = $state(false)
  let humanError = $state<string | null>(null)
  let humanReceipt = $state<HumanReceipt | null>(null)
  /** FR-69: the temporary rule the last receipt confirmed. */
  let humanRule = $state<TempRuleEntry | null>(null)
  /** FR-138: a proposal approved on its attention card, kept until the receipt matches so it can be retried. */
  let pendingApproval = $state<{ token: string; id: string; input: PublicationApproval } | null>(null)
  let approvalBusy = $state(false)
  let approvalError = $state<string | null>(null)
  let approvalResult = $state<string | null>(null)
  /** Which one-step action ran last, so the list shows its status (FR-138). */
  let lastQuick = $state<'answer' | 'approve' | null>(null)
  /** FR-130: interruptions already notified on this machine, by `interruptionKey`. */
  let notified: { token: string | null; keys: Set<string> } = { token: null, keys: new Set() }
  let mainResolution = $state<{ token: string; commit: string } | null>(null)
  // FR-130: only "now" items interrupt, through a system notification the
  // App raises (the daemon never does). Items already present when a project
  // is opened are on screen, so only ones that arrive later notify.
  $effect(() => {
    const token = sample || projectState?.status !== 'ready' ? null : project?.token ?? null
    if (!token) return
    const items = runtime.allAttention
    if (notified.token !== token) {
      notified = { token, keys: new Set(newInterruptions(items, new Set()).map(interruptionKey)) }
      return
    }
    for (const item of newInterruptions(items, notified.keys)) {
      notified.keys.add(interruptionKey(item))
      const { title, body } = notification(item)
      void client.notify?.(title, body).catch(() => {})
    }
  })
  let showMainResolution = $state(false)
  let mainResolutionBlocked = $state(false)
  let showProposals = $state(false)
  let proposalsOpened = $state(false)
  let proposalBlocked = $state(false)
  let showTaskCreate = $state(false)
  let showControl = $state(false)
  let taskOptions = $state<{ baseline: string; modules: { id: string; title: string; paths: string[] }[] } | null>(null)
  let taskOptionsLoading = $state(false)
  let taskOptionsGeneration = 0
  type TaskControlRequest = { token: string } & (
    { kind: 'task'; input: { request_id: string; task: HumanTaskDraft } }
    | { kind: 'control'; input: ProjectControlRequest })
  let pendingTaskControl = $state<TaskControlRequest | null>(null)
  let taskControlBusy = $state(false)
  let taskControlError = $state<string | null>(null)
  let taskControlReceipt = $state<string | null>(null)
  let showLedger = $state(false)
  let taskFocus = $state<string | null>(null)
  let pendingDelegation = $state<{ token: string; action: DelegationAction } | null>(null)
  let delegationBusy = $state(false)
  let delegationError = $state<string | null>(null)
  let delegationResult = $state<string | null>(null)
  let ruleOffer = $state<RuleOffer | null>(null)
  // FR-181–183: ideas are jotted and promoted only here, in the App, through the daemon.
  type IdeaWrite = { token: string } & ({ kind: 'add'; input: IdeaRequest } | { kind: 'promote'; input: IdeaPromotion })
  let pendingIdea = $state<IdeaWrite | null>(null)
  let ideaBusy = $state(false)
  let ideaError = $state<string | null>(null)
  let ideaReceipt = $state<string | null>(null)
  let promotingIdea = $state<IdeaEntry | null>(null)
  let promotedThisVisit = $state(0)
  let showIdeas = $state(false)
  let ideas = $derived(projectState?.status === 'ready' ? ideasFrom(projectState.projection) : [])
  let hasGoal = $derived(ideas.some((idea) => idea.promoted?.as === 'project'))
  let ideaHint = $derived(followUpHint(promotedThisVisit))
  // FR-134, FR-136: follow-ups are added and completed, and a resurfaced
  // object is settled, only here in the App, through the daemon.
  type RhythmWrite = { token: string } & ({ kind: 'add'; input: FollowUpRequest }
    | { kind: 'done'; input: { request_id: string; follow_up: string } } | { kind: 'seen'; input: { request_id: string; object: string } })
  let pendingRhythm = $state<RhythmWrite | null>(null)
  let rhythmBusy = $state(false)
  let rhythmError = $state<string | null>(null)
  let rhythmReceipt = $state<string | null>(null)
  let lastRhythm = $state<RhythmWrite['kind'] | null>(null)
  let followUpTargets = $derived(runtime.rhythm ? runtime.rhythm.objects.map((object, index) => ({ id: object.id,
    title: index === 0 ? '项目' : runtime.delegation?.objects.find((entry) => entry.id === object.id)?.title ?? object.id })) : [])
  // FR-141–146: frames are placed, moved and resized only here, in the App, through the daemon.
  // H2: a person's card writes, rule changes and decisions on waiting items go the same way.
  // H3: so do the places a person drags cards to (FR-149).
  type FrameWrite = { token: string; what: string } & ({ kind: 'create'; input: FrameRequest } | { kind: 'place'; input: PlaceFrameRequest }
    | { kind: 'card'; input: CardRequest } | { kind: 'rules'; input: FrameRulesRequest } | { kind: 'resolve'; input: FrameDecisionRequest }
    | { kind: 'pin'; input: PinCardsRequest })
  let pendingFrame = $state<FrameWrite | null>(null)
  let frameBusy = $state(false)
  let frameError = $state<string | null>(null)
  let frameReceipt = $state<string | null>(null)
  let showFrames = $state(false)
  let templates = $state<FrameTemplate[] | null>(null)
  let templatesLoading = $state(false)
  let templatesError = $state<string | null>(null)
  let templatesGeneration = 0
  let droppedTemplate = $state<{ id: string; at: { x: number; y: number } } | null>(null)
  /** Where the person last put a frame, until the live state has that event (`seq: null` while unconfirmed). */
  let placedLocally = $state<Record<string, { place: FramePlace; seq: number | null }>>({})
  /** Where the person last dragged cards in a frame, until the live state has that event. */
  let pinnedLocally = $state<Record<string, { pins: PinCardsRequest['pins']; seq: number | null }>>({})
  /** FR-144 方式 2: nodes the person picked to turn into a frame, waiting for a template. */
  let converting = $state<{ token: string; nodes: CanvasNode[]; edges: CanvasEdge[] } | null>(null)
  let shellWidth = $state(0)
  let shellHeight = $state(0)
  let frameMode = $derived(!sample && !candidate && !draft && !!project && !failedSelection && projectState?.status === 'ready')
  let frames = $derived.by(() => {
    if (!frameMode || projectState?.status !== 'ready') return []
    const seen = projectState.seq ?? 0
    return framesFrom(projectState.projection, projectState.frames).map((frame) => {
      const local = placedLocally[frame.id]
      const pinned = pinnedLocally[frame.id]
      const placed = local && (local.seq === null || seen < local.seq) ? { ...frame, place: local.place } : frame
      return pinned && (pinned.seq === null || seen < pinned.seq) ? withPins(placed, pinned.pins) : placed
    })
  })
  // FR-151 (after FR-50): changes inside frames show at most once a second; a frame
  // placed, moved, renamed or a card dragged, and the person's own writes, show at once.
  let shownFrames = $state<FrameEntry[]>([])
  let frameTimer: ReturnType<typeof setTimeout> | undefined
  let flushFrames = false
  $effect(() => {
    const next = frames
    untrack(() => {
      const change = frameChange(shownFrames, next)
      if (change === 'same') return
      if (change === 'now' || flushFrames) {
        clearTimeout(frameTimer)
        frameTimer = undefined
        flushFrames = false
        shownFrames = next
      } else if (frameTimer === undefined) {
        frameTimer = setTimeout(() => { frameTimer = undefined; shownFrames = frames }, 1000)
      }
    })
  })
  let badges = $derived(frames.length && projectState?.status === 'ready'
    ? frameBadges(signalEntries((projectState.projection as { TowerApi?: unknown } | undefined)?.TowerApi), frames, Date.now()) : {})
  let control = $derived.by((): ProjectControlState => {
    const projection = projectState?.status === 'ready' ? projectState.projection as Record<string, unknown> : {}
    const value = projection?.ProjectControl as ProjectControlState | undefined
    return value?.paused === true && typeof value.pause_event === 'string' ? value
      : { paused: false, pause_event: null, scope: 'all', resume_requests: [] }
  })
  let humanQuestion = $derived(humanContext?.kind === 'answer' ? humanContext.question : undefined)
  let humanScope = $derived(humanContext?.kind === 'answer' ? `回答模块 ${humanContext.question.binding.module} 的问题`
    : humanContext?.scope.kind === 'module' ? `向模块 ${humanContext.scope.module} 插话`
    : humanContext?.scope.kind === 'task' ? `向任务 ${humanContext.scope.task} 插话` : '向当前项目的活跃运行插话')
  /** FR-69: active temporary rules that reach the runs this prompt would reach. */
  let humanRules = $derived(humanContext?.kind === 'prompt' && projectState?.status === 'ready'
    ? activeTempRules((projectState.projection as { TempRule?: unknown } | undefined)?.TempRule, humanContext.scope, Date.now()) : [])
  let awaitingLiveState = $derived(projectState?.status === 'ready' && minimumSeq > (projectState.seq ?? 0))
  let interactionWriteBlocked = $derived(humanBusy || !!pendingHuman || mainResolutionBlocked || proposalBlocked || taskControlBusy || !!pendingTaskControl || delegationBusy || !!pendingDelegation || ideaBusy || !!pendingIdea || rhythmBusy || !!pendingRhythm || approvalBusy || !!pendingApproval || frameBusy || !!pendingFrame)
  let writeBlocked = $derived(editBusy || !!pendingEdit || verificationBusy || !!pendingVerification || publicationBusy || publicationAttempted || recoveryBusy || recoveryAttempted || interactionWriteBlocked)
  let canvas: { canvasPoint: (x: number, y: number) => { x: number; y: number } } | undefined = $state()
  let detailReport = $derived(draft ? baselineReport(draft) : (candidate ? report : null))
  let statuses: Record<string, CardStatus> = $derived.by(() => {
    if (sample) return {}
    const result = draft && baselineEvidenceError(draft)
      ? Object.fromEntries((draft.review?.modules ?? draft.scan.modules).map((module) => [module.id, 'unverifiable' as const]))
      : moduleStatuses(detailReport, !draft && jobActive && activeJob?.id === detailReport?.id)
    for (const id of draft?.review?.pending_modules ?? []) result[id] = 'pending'
    return result
  })
  let showScan = $state(false)
  // J4: entering a draft from the approved baseline, and its read-only comparison.
  let showCompare = $state(false)
  let comparison = $state<DraftComparison | null>(null)
  let comparisonLoading = $state(false)
  let comparisonError = $state<string | null>(null)
  let baselineDraftBusy = $state(false)
  let baselineDraftError = $state<string | null>(null)
  let displayed = $derived(sample ? sampleView(sample) : draft?.view ?? (candidate && report ? report.view : view))
  let selectedModule = $derived(detailReport && selected
    ? detailReport.modules.find((module) => module.id === selected?.object_id || module.id === selected?.id)
    : undefined)

  function stopPolling() {
    scanGeneration += 1
    clearTimeout(scanTimer)
    scanTimer = undefined
    scanning = false
  }

  onDestroy(() => { clearTimeout(verificationTimer); clearTimeout(frameTimer); stopPolling(); stopLiveUpdates(); draftGeneration += 1; selectionToken = null })

  function stopLiveUpdates() {
    liveGeneration += 1
    liveReadGeneration += 1
    const subscription = liveSubscription
    liveSubscription = null
    if (subscription) void client.unsubscribeProject?.(subscription).catch(() => {})
    liveInFlight = false
    stateLoading = false
    connectionError = null
  }

  function acceptLiveState(result: ProjectState) {
    if (result.status === 'ready') {
      if (!Number.isSafeInteger(result.seq) || result.seq! < 0) {
        stateError = '实时状态缺少有效事件序号，请刷新重试。'
        return false
      }
      if (result.seq! < minimumSeq) return false
      minimumSeq = result.seq!
    }
    projectState = result
    stateError = null
    return true
  }

  async function readLiveState() {
    if (sample || !project || failedSelection || !client.projectState || liveInFlight || project.token !== selectionToken) return
    const request = liveGeneration
    const read = ++liveReadGeneration
    const token = project.token
    liveInFlight = true
    stateLoading = true
    try {
      const result = await client.projectState(token)
      if (request !== liveGeneration || read !== liveReadGeneration || token !== selectionToken || sample) return
      acceptLiveState(result)
    } catch (cause) {
      if (request === liveGeneration && read === liveReadGeneration && token === selectionToken && !sample) stateError = String(cause)
    } finally {
      if (request === liveGeneration && read === liveReadGeneration && token === selectionToken && !sample) {
        liveInFlight = false
        stateLoading = false
      }
    }
  }

  // Writes invalidate outstanding reads and reject buffered frames older than
  // their canonical receipt. Keep the single live subscription in place.
  function refreshLiveState(seq = minimumSeq) {
    minimumSeq = Math.max(minimumSeq, seq)
    liveReadGeneration += 1
    liveInFlight = false
    void readLiveState()
  }

  function startLiveUpdates() {
    stopLiveUpdates()
    if (sample || !project || failedSelection || project.token !== selectionToken) return
    const token = project.token
    const request = liveGeneration
    void readLiveState()
    if (!client.subscribeProject) return
    const subscription = humanRequestId()
    liveSubscription = subscription
    void client.subscribeProject(token, subscription, (message) => {
      if (request !== liveGeneration || subscription !== liveSubscription || token !== selectionToken || sample
        || message.token !== token || message.subscription !== subscription) return
      if (message.status === 'disconnected') {
        connectionError = '实时连接中断，正在重连；当前显示上次状态。'
        return
      }
      if (message.status !== 'connected' || !message.state) return
      if (acceptLiveState(message.state)) {
        connectionError = null
        liveReadGeneration += 1
        liveInFlight = false
        stateLoading = false
      }
    }).then(() => {
      if (request !== liveGeneration || subscription !== liveSubscription || token !== selectionToken || sample) {
        return client.unsubscribeProject?.(subscription)
      }
    }).catch(() => {
      if (request === liveGeneration && token === selectionToken && !sample) {
        connectionError = '实时连接启动失败，请刷新重试。'
      }
    })
  }

  function selectCandidate(value: boolean) {
    if (sample || writeBlocked) return
    draft = null
    candidate = value
    fitVersion += 1
    closeOverlays()
  }

  async function receiveScan(job: ScanJob, token: string, request: number) {
    if (request !== scanGeneration || token !== selectionToken) return
    activeJob = job
    if (job.report) {
      const firstReport = report?.id !== job.report.id
      report = updateCandidate(report, job.report)
      if (selected && candidate) selected = report.view.nodes.find((node) => node.id === selected?.id) ?? selected
      if (firstReport) selectCandidate(true)
    }
    if (job.status === 'running') {
      cancelling = cancelling || job.cancel_requested === true || job.phase === 'cancelling'
      scanning = true
      scanTimer = setTimeout(async () => {
        scanTimer = undefined
        try {
          await receiveScan(await client.pollScan(token, job.id), token, request)
        } catch (cause) {
          failScan(cause, request)
        }
      }, 2000)
      return
    }
    scanning = false
    cancelling = false
    if (job.report?.verification && !job.report.verification.cleanup_complete) throw new Error(`验证清理失败，不能接受通过结果。${job.error || ''}`)
    if (job.status === 'failed') throw new Error(job.error || '扫描未完成，请重试。')
    if (job.status === 'ready' && !job.report) throw new Error('扫描结果缺少候选报告，请重试。')
    scanError = null
  }

  function failScan(cause: unknown, request: number) {
    if (request !== scanGeneration) return
    scanning = false
    cancelling = false
    scanError = String(cause)
  }

  async function resumeScan() {
    if (sample || !project || !activeJob || busy || writeBlocked) return
    menu = null
    if (draft) selectCandidate(true)
    stopPolling()
    const request = scanGeneration
    const token = project.token
    scanning = true
    scanError = null
    try {
      await receiveScan(await client.pollScan(token, activeJob.id), token, request)
    } catch (cause) {
      failScan(cause, request)
    }
  }

  async function cancelScan() {
    if (sample || !project || !activeJob || !jobActive || cancelling) return
    stopPolling()
    const request = scanGeneration
    const token = project.token
    const id = activeJob.id
    cancelling = true
    scanning = true
    scanError = null
    try {
      await receiveScan(await client.cancelScan(token, id), token, request)
    } catch (cause) {
      failScan(cause, request)
    }
  }

  function startScan() {
    return beginScan(false)
  }

  /** J4: scan the current commit inside the approved modules instead of clustering again. */
  function startBaselineScan() {
    return beginScan(true)
  }

  async function beginScan(fromBaseline: boolean) {
    if (sample || writeBlocked || !project || !view?.docs_dir || busy || scanning || startPending || jobActive || failedSelection) return
    menu = null
    closeOverlays()
    if (draft) selectCandidate(false)
    stopPolling()
    const request = scanGeneration
    const token = project.token
    activeJob = null
    startPending = true
    scanning = true
    scanError = null
    try {
      const job = fromBaseline ? await client.startScan(token, true) : await client.startScan(token)
      // Refresh may stop observing before the start acknowledgement returns.
      // Keep the ID so the existing daemon job can be resumed, not duplicated.
      if (request !== scanGeneration && token === selectionToken) activeJob = job
      else await receiveScan(job, token, request)
    } catch (cause) {
      failScan(cause, request)
    } finally {
      if (token === selectionToken) startPending = false
    }
  }

  function closeOverlays() {
    showTaskCreate = false
    showLedger = false
    taskFocus = null
    showControl = false
    showProposals = false
    showMainResolution = false
    showHuman = false
    showTasks = false
    showModuleTasks = false
    draftGeneration += 1
    draftLoading = false
    showDrafts = false
    showDraft = false
    editing = false
    showReference = false
    menu = null
    selected = null
    showProject = false
    showScan = false
    showCompare = false
    showExecution = false
    showIdeas = false
    showFrames = false
    if (pendingIdea?.kind !== 'promote') promotingIdea = null
  }

  async function listDrafts() {
    if (sample || !project || busy || failedSelection || writeBlocked) return
    closeOverlays()
    showDrafts = true
    drafts = []
    draftError = null
    draftLoading = true
    const request = draftGeneration
    const token = project.token
    try {
      if (!client.listBaselines) throw new Error('当前客户端不支持综合草稿，请更新 App。')
      const result = await client.listBaselines(token)
      if (request !== draftGeneration || token !== selectionToken) return
      drafts = result
    } catch (cause) {
      if (request === draftGeneration && token === selectionToken) draftError = String(cause)
    } finally {
      if (request === draftGeneration && token === selectionToken) draftLoading = false
    }
  }

  async function loadDraft(id: string) {
    if (sample || !project || busy || failedSelection || writeBlocked) return
    stopPolling()
    const request = ++draftGeneration
    const token = project.token
    draftLoading = true
    draftError = null
    try {
      if (!client.loadBaseline) throw new Error('当前客户端不支持综合草稿，请更新 App。')
      const loaded = await client.loadBaseline(token, id)
      if (request !== draftGeneration || token !== selectionToken) return
      const changed = draft?.id !== loaded.id
      // Reopening evidence for the same draft does not move its existing nodes.
      const previousNodes = new Map(draft?.view.nodes.map((node) => [node.id, node]) ?? [])
      draft = !changed && draft ? {
        ...loaded,
        view: { ...loaded.view, nodes: loaded.view.nodes.map((node) => {
          const previous = previousNodes.get(node.id)
          return previous ? { ...node, x: previous.x, y: previous.y, width: previous.width, height: previous.height } : node
        }) },
      } : loaded
      if (changed) { publication = null; publicationError = null }
      candidate = false
      if (changed) fitVersion += 1
      closeOverlays()
    } catch (cause) {
      if (request === draftGeneration && token === selectionToken) draftError = String(cause)
    } finally {
      if (request === draftGeneration && token === selectionToken) draftLoading = false
    }
  }

  /** J4: the daemon turns the finished from-baseline scan into an unreviewed draft. */
  async function createBaselineDraft() {
    if (sample || !project || !report?.origin || busy || writeBlocked || baselineDraftBusy || failedSelection) return
    const token = project.token
    const scan = report.id
    baselineDraftBusy = true
    baselineDraftError = null
    try {
      if (!client.draftFromScan) throw new Error('当前客户端不支持从基线进入草稿，请更新 App。')
      const created = await client.draftFromScan(token, scan)
      if (token !== selectionToken) return
      await loadDraft(created.id)
      if (draft?.id === created.id) await openComparison()
    } catch (cause) {
      if (token === selectionToken) baselineDraftError = String(cause)
    } finally {
      if (token === selectionToken) baselineDraftBusy = false
    }
  }

  async function openComparison() {
    if (sample || !project || !draft?.scan.origin || failedSelection) return
    const token = project.token
    const id = draft.id
    closeOverlays()
    showCompare = true
    comparison = comparison?.comparison.draft_id === id ? comparison : null
    comparisonLoading = true
    comparisonError = null
    try {
      if (!client.compareBaseline) throw new Error('当前客户端不支持草稿对比，请更新 App。')
      const result = await client.compareBaseline(token, id)
      if (token !== selectionToken || draft?.id !== id) return
      comparison = result
    } catch (cause) {
      if (token === selectionToken) comparisonError = String(cause)
    } finally {
      if (token === selectionToken) comparisonLoading = false
    }
  }

  function draftTitle(id: string) {
    return draft?.view.nodes.find((node) => node.id === id)?.title ?? id
  }

  async function load(selection: ProjectSelection, request: number) {
    try {
      const loaded = await client.loadProject(selection.token)
      if (request !== generation) return
      const changedProject = project?.path !== selection.path
      if (changedProject) {
        if (project) recordVisit(project.path, Date.now())
        visit = lastVisit(selection.path, Date.now())
        digestOpen = true
      }
      project = selection
      view = loaded
      error = null
      failedSelection = null
      if (changedProject) fitVersion += 1
      closeOverlays()
      startLiveUpdates()
    } catch (cause) {
      if (request === generation) {
        error = String(cause)
        failedSelection = selection
      }
    } finally {
      if (request === generation) busy = false
    }
  }

  async function chooseProject() {
    if (sample || writeBlocked) return
    const request = ++generation
    closeOverlays()
    busy = true
    try {
      const selection = await client.chooseProject()
      if (request !== generation) return
      if (!selection) {
        busy = false
        return
      }
      selectionToken = selection.token
      stopLiveUpdates()
      taskPositions.clear()
      stopPolling()
      activeJob = null
      startPending = false
      cancelling = false
      report = null
      draft = null
      editError = null
      pendingEdit = null
      pendingVerification = null
      verificationJob = null
      verificationError = null
      publication = null
      publicationError = null
      publicationAttempted = false
      published = null
      projectState = null
      minimumSeq = 0
      proposalsOpened = false
      runDetails = {}
      detailsGeneration += 1
      detailsLoading = null
      detailsError = undefined
      humanContext = null
      humanError = null
      humanReceipt = null
      humanRule = null
      approvalError = null
      approvalResult = null
      lastQuick = null
      stateError = null
      recovery = null
      recoveryError = null
      recoveryAttempted = false
      recoveryCommit = null
      clearTimeout(verificationTimer)
      drafts = []
      candidate = false
      scanError = null
      promotingIdea = null
      promotedThisVisit = 0
      ideaError = null
      ideaReceipt = null
      rhythmError = null
      rhythmReceipt = null
      lastRhythm = null
      frameError = null
      frameReceipt = null
      templates = null
      templatesError = null
      templatesGeneration += 1
      templatesLoading = false
      droppedTemplate = null
      placedLocally = {}
      pinnedLocally = {}
      converting = null
      // Keep the last successful canvas until its replacement is fully loaded.
      await load(selection, request)
    } catch (cause) {
      if (request === generation) {
        error = String(cause)
        busy = false
      }
    }
  }

  async function refresh() {
    if (sample || writeBlocked) return
    closeOverlays()
    if (!project) {
      await chooseProject()
      return
    }
    stopPolling()
    if (candidate || draft) selectCandidate(false)
    const request = ++generation
    busy = true
    await load(project, request)
  }

  async function retry() {
    if (sample || writeBlocked) return
    if (!failedSelection) {
      await refresh()
      return
    }
    const request = ++generation
    busy = true
    await load(failedSelection, request)
  }

  async function saveEdit(action?: ReviewAction) {
    if (sample || !project || !draft || interactionWriteBlocked || editBusy || pendingVerification || verificationBusy || publicationBusy || publicationAttempted || recoveryBusy || recoveryAttempted || draft.stale || busy) return
    if (action && pendingEdit) return
    const retryingUnknown = !!pendingEdit
    const request = pendingEdit ?? (action ? { token: project.token, id: draft.id, action: JSON.parse(JSON.stringify(action)) as ReviewAction } : null)
    if (!request || request.token !== selectionToken) return
    pendingEdit = request
    publication = null
    publicationError = null
    editBusy = true
    editError = null
    try {
      if (!client.reviewBaseline) throw new Error('操作已拒绝：当前客户端不支持编辑草稿，请更新 App。')
      const updated = await client.reviewBaseline(request.token, request.id, request.action)
      if (selectionToken !== request.token) return
      draft = updated
      if (selected) selected = updated.view.nodes.find((node) => node.id === selected?.id) ?? null
      pendingEdit = null
      showReference = false
    } catch (cause) {
      if (selectionToken !== request.token) return
      editError = String(cause)
      if (!retryingUnknown && editError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingEdit = null
      // Reset a transient dragged position to the last confirmed canvas.
      draft = { ...draft, view: { ...draft.view, nodes: draft.view.nodes.map((node) => ({ ...node })) } }
    } finally {
      if (selectionToken === request.token) editBusy = false
    }
  }

  async function receiveVerification(job: BaselineVerificationJob) {
    const request = pendingVerification
    if (!request || request.token !== selectionToken || job.id !== request.id || job.parent_id !== request.parent) return
    verificationJob = job
    verificationBusy = false
    verificationError = job.error
    clearTimeout(verificationTimer)
    if (job.status === 'running') {
      verificationTimer = setTimeout(async () => {
        try {
          if (!client.pollBaselineVerification) throw new Error('客户端不支持查询审阅验证。')
          await receiveVerification(await client.pollBaselineVerification(request.token, request.id))
        } catch (cause) { if (selectionToken === request.token) verificationError = String(cause) }
      }, 1500)
      return
    }
    if (job.phase === 'publishing') return
    if (job.result_id) {
      if (!client.loadBaseline) throw new Error('客户端不支持读取草稿。')
      const result = await client.loadBaseline(request.token, job.result_id)
      if (selectionToken !== request.token || pendingVerification?.id !== request.id) return
      draft = result
      if (selected) selected = result.view.nodes.find((node) => node.id === selected?.id) ?? null
      // A fast-layer result never changes the canvas viewport or node positions.
    }
    pendingVerification = null
  }

  async function verifyDraft() {
    if (sample || !project || !draft || interactionWriteBlocked || verificationBusy || editBusy || pendingEdit || publicationBusy || publicationAttempted || recoveryBusy || recoveryAttempted || busy || draft.stale || jobActive) return
    const retrying = !!pendingVerification
    const request = pendingVerification ?? { token: project.token, parent: draft.id, id: crypto.randomUUID() }
    pendingVerification = request
    publication = null
    publicationError = null
    verificationBusy = true
    verificationError = null
    clearTimeout(verificationTimer)
    try {
      if (!client.verifyBaseline) throw new Error('操作已拒绝：当前客户端不支持审阅验证。')
      await receiveVerification(await client.verifyBaseline(request.token, request.parent, request.id))
    } catch (cause) {
      if (selectionToken !== request.token) return
      verificationError = String(cause)
      if (!retrying && verificationError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingVerification = null
    } finally { if (selectionToken === request.token) verificationBusy = false }
  }

  async function cancelVerification() {
    const request = pendingVerification
    if (!request || verificationBusy) return
    verificationBusy = true
    clearTimeout(verificationTimer)
    try {
      if (!client.cancelBaselineVerification) throw new Error('客户端不支持取消审阅验证。')
      await receiveVerification(await client.cancelBaselineVerification(request.token, request.id))
    } catch (cause) { if (selectionToken === request.token) verificationError = String(cause) }
    finally { if (selectionToken === request.token) verificationBusy = false }
  }

  async function preparePublication() {
    if (sample || !project || !draft || writeBlocked || draft.stale || busy || jobActive) return
    const token = project.token
    const id = draft.id
    publicationBusy = true
    publicationError = null
    try {
      if (!client.preparePublication) throw new Error('当前客户端不支持版本出版。')
      const preview = await client.preparePublication(token, id)
      if (token !== selectionToken || draft?.id !== id) return
      if (preview.draft_id !== id || preview.project_id !== draft.submission.project_id
        || preview.source_revision !== draft.submission.source_revision) throw new Error('批准预览与当前草稿不一致，请重新读取。')
      // Keep only the review metadata; publication file bytes stay in the daemon.
      publication = { token, preview: {
        request_id: preview.request_id, draft_id: preview.draft_id, project_id: preview.project_id,
        source_revision: preview.source_revision, target: preview.target,
        prepared_at: preview.prepared_at, content_hash: preview.content_hash,
      } }
    } catch (cause) { if (token === selectionToken) publicationError = String(cause) }
    finally { if (token === selectionToken) publicationBusy = false }
  }

  async function approvePublication() {
    const request = publication
    if (sample || !request || publicationBusy || !draft || request.preview.draft_id !== draft.id
      || request.token !== selectionToken || interactionWriteBlocked || editBusy || pendingEdit || verificationBusy || pendingVerification || recoveryBusy || recoveryAttempted) return
    publicationAttempted = true
    publicationBusy = true
    publicationError = null
    const approval: PublicationApproval = {
      request_id: request.preview.request_id, target: request.preview.target,
      prepared_at: request.preview.prepared_at, content_hash: request.preview.content_hash,
    }
    try {
      if (!client.approvePublication) throw new Error('当前客户端不支持批准版本。')
      const receipt = await client.approvePublication(request.token, request.preview.draft_id, approval)
      if (request.token !== selectionToken) return
      if (receipt.target !== approval.target || receipt.content_hash !== approval.content_hash
        || !receipt.commit || !receipt.tag || !receipt.snapshot_commit) throw new Error('出版回执与原批准内容不一致；请重试原批准确认结果。')
      published = receipt
      publication = null
      publicationAttempted = false
      draft = null
      candidate = false
      closeOverlays()
      showProject = true
      startLiveUpdates()
      try {
        const refreshed = await client.loadProject(request.token)
        if (request.token === selectionToken) view = refreshed
      } catch (cause) {
        if (request.token === selectionToken) stateError = `版本已出版，画布读取失败：${String(cause)}`
      }
    } catch (cause) {
      if (request.token === selectionToken) publicationError = String(cause)
    } finally { if (request.token === selectionToken) publicationBusy = false }
  }

  async function openProjectState() {
    if (sample || !project || failedSelection) return
    closeOverlays()
    showProject = true
    await readLiveState()
  }

  function openTasks(focus: string | null = null) {
    if (sample || !project || failedSelection) return
    closeOverlays()
    if (!pendingDelegation) { delegationError = null; delegationResult = null }
    taskFocus = focus
    showTasks = true
    void readLiveState()
  }

  async function loadRunDetails(id: string) {
    if (sample || !project || failedSelection || !client.runDetails) return
    const token = project.token
    const request = ++detailsGeneration
    detailsLoading = id
    detailsError = undefined
    try {
      const value = await client.runDetails(token, id)
      if (token !== selectionToken || request !== detailsGeneration || sample) return
      if (value.run_id !== id) throw new Error('运行详情与所选运行不一致。')
      runDetails = { ...runDetails, [id]: value }
    } catch (cause) {
      if (token === selectionToken && request === detailsGeneration) detailsError = String(cause)
    } finally {
      if (token === selectionToken && request === detailsGeneration) detailsLoading = null
    }
  }

  function openMainResolution(commit: string) {
    if (sample || !project || failedSelection || (writeBlocked && !mainResolutionBlocked)) return
    if (mainResolutionBlocked && mainResolution?.commit !== commit) return
    closeOverlays()
    if (!mainResolutionBlocked) mainResolution = { token: project.token, commit }
    showMainResolution = true
  }

  function openProposals() {
    if (sample || !project || failedSelection || (writeBlocked && !proposalBlocked)) return
    closeOverlays()
    proposalsOpened = true
    showProposals = true
  }

  function openAttention(item: RuntimeAttention) {
    if (item.question) openHuman({ kind: 'answer', question: item.question })
    else if (item.commit) openMainResolution(item.commit)
    else if (item.proposal) openProposals()
    else openTasks(item.delegation ?? null)
  }

  function openLedger() {
    if (sample || !project || failedSelection) return
    closeOverlays()
    if (!pendingDelegation) { delegationError = null; delegationResult = null }
    showLedger = true
    void readLiveState()
  }

  /** Reopens where a pending delegation action can be retried. */
  function openPendingDelegation() {
    const action = pendingDelegation?.action
    if (!action) return
    if (action.kind === 'rename' || action.kind === 'level') openLedger()
    else if (action.kind === 'signal' || action.kind === 'ask' || action.kind === 'withdraw') openTasks()
    else openTasks(actionObject(action))
  }

  const delegationMessages: Record<DelegationAction['kind'], string> = {
    finalize: '委托卡已定稿，任务可以领取。', used: '已记下"我用过"。', confirm: '验收已记录。',
    send_back: '已退回，受托者下次领取时会看到理由。', rule: '规则已记录。',
    waive: '豁免已记录，到期自动失效，门禁恢复拦截。', rename: '工作类别已改名。', level: '委托级别已设定。',
    signal: '提醒已处理。', ask: '已发出；收件人在下次调用 towermd 时收到。', withdraw: '请求已撤回。',
  }

  async function submitDelegation(intent: DelegationIntent) {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    const request_id = humanRequestId()
    const action = (ledgerAction(intent)
      ? { kind: intent.kind, input: { request_id, data: intent.data } }
      : { kind: intent.kind, input: { request_id, object: intent.object, data: intent.data } }) as DelegationAction
    delegationResult = null
    lastDelegationKind = action.kind
    pendingDelegation = { token: project.token, action }
    await sendDelegation(false)
  }

  function dispatchDelegation(token: string, action: DelegationAction): Promise<DelegationReceipt> {
    const unsupported = () => Promise.reject(new Error('操作已拒绝：当前客户端不支持此委托操作，请更新 App。'))
    switch (action.kind) {
      case 'finalize': return client.finalizeDelegation?.(token, action.input) ?? unsupported()
      case 'used': return client.recordAcceptanceUsed?.(token, action.input) ?? unsupported()
      case 'confirm': return client.confirmAcceptance?.(token, action.input) ?? unsupported()
      case 'send_back': return client.sendBackAcceptance?.(token, action.input) ?? unsupported()
      case 'rule': return client.recordRule?.(token, action.input) ?? unsupported()
      case 'waive': return client.waiveGate?.(token, action.input) ?? unsupported()
      case 'rename': return client.renameWorkKind?.(token, action.input) ?? unsupported()
      case 'level': return client.setDelegationLevel?.(token, action.input) ?? unsupported()
      case 'signal': return client.dismissSignal?.(token, action.input) ?? unsupported()
      case 'ask': return client.askAbout?.(token, action.input) ?? unsupported()
      case 'withdraw': return client.withdrawRequest?.(token, action.input) ?? unsupported()
    }
  }

  async function sendDelegation(retrying = true) {
    const request = pendingDelegation
    if (!request || sample || delegationBusy || request.token !== selectionToken) return
    delegationBusy = true
    delegationError = null
    try {
      const { action } = request
      const receipt = await dispatchDelegation(request.token, action)
      if (request.token !== selectionToken) return
      const expected = receiptObject(action)
      if (receipt.request_id !== action.input.request_id || receipt.event !== action.input.request_id || receipt.obj !== expected
        || receipt.status !== 'accepted' || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0) {
        throw new Error('回执与原请求不一致，请重试原请求确认。')
      }
      delegationResult = delegationMessages[action.kind]
      pendingDelegation = null
      if (action.kind === 'send_back') {
        const object = runtime.delegation?.objects.find((item) => item.id === action.input.object)
        ruleOffer = object ? { object: object.id, kind: object.work_kind, source: receipt.event, text: action.input.data.reason } : null
      } else if (action.kind === 'rule') ruleOffer = null
      refreshLiveState(receipt.seq)
    } catch (cause) {
      if (request.token === selectionToken) {
        delegationError = String(cause)
        if (!retrying && delegationError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingDelegation = null
      }
    } finally { if (request.token === selectionToken) delegationBusy = false }
  }

  /** FR-138: confirm a spot-checked attempt on its card through the same human-only route as the acceptance card. */
  function quickConfirm(item: RuntimeAttention, review_ms: number) {
    if (!item.confirm) return
    void submitDelegation({ kind: 'confirm', object: item.confirm.object, data: { review_ms, unproven: [...item.confirm.unproven] } })
  }

  /** FR-138: answer a question on its card; recorded "for this time only", as the answer panel's default. */
  async function quickAnswer(item: RuntimeAttention, text: string) {
    if (sample || !project || !item.question || pendingHuman || humanBusy || writeBlocked || awaitingLiveState || !text.trim()) return
    const context: HumanContext = { kind: 'answer', question: item.question }
    lastQuick = 'answer'
    humanContext = context
    humanError = null
    humanReceipt = null
    pendingHuman = { token: project.token, context, kind: 'answer',
      input: { request_id: humanRequestId(), hold: item.question.draft.id, text, remember_as: 'temporary', targets: [] } }
    await sendHuman(false)
  }

  /**
   * FR-138: approve a submitted proposal on its card. The publication is
   * prepared and checked against the proposal exactly as the proposal panel
   * does, then approved through the same human-only route.
   */
  async function quickApprove(item: RuntimeAttention) {
    const expected = item.approve
    if (sample || !project || !expected || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    const token = project.token
    lastQuick = 'approve'
    approvalError = null
    approvalResult = null
    approvalBusy = true
    try {
      if (!client.prepareProposal) throw new Error('当前客户端不支持提案出版。')
      const preview = await client.prepareProposal(token, item.id)
      if (token !== selectionToken) return
      if (preview.draft_id !== expected.draft_id || preview.project_id !== expected.project_id
        || preview.source_revision !== expected.source_revision || !preview.request_id || !preview.content_hash) {
        throw new Error('提案出版预览与原件不一致，请打开提案重新审阅。')
      }
      pendingApproval = { token, id: item.id, input: {
        request_id: preview.request_id, target: preview.target, prepared_at: preview.prepared_at, content_hash: preview.content_hash,
      } }
    } catch (cause) { if (token === selectionToken) approvalError = String(cause) }
    finally { if (token === selectionToken) approvalBusy = false }
    await sendApproval(false)
  }

  async function sendApproval(retrying = true) {
    const request = pendingApproval
    if (!request || approvalBusy || sample || request.token !== selectionToken) return
    approvalBusy = true
    approvalError = null
    try {
      if (!client.approveProposal) throw new Error('操作已拒绝：当前客户端不支持批准提案。')
      const receipt = await client.approveProposal(request.token, request.id, request.input)
      if (request.token !== selectionToken) return
      if (receipt.content_hash !== request.input.content_hash || receipt.target !== request.input.target
        || !receipt.commit || !receipt.tag || !receipt.snapshot_commit) throw new Error('出版回执不匹配，请重试原确认。')
      pendingApproval = null
      approvalResult = `已出版 ${receipt.tag} · ${receipt.commit}`
      void refreshAuthority()
    } catch (cause) {
      if (request.token === selectionToken) {
        approvalError = String(cause)
        if (!retrying && approvalError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingApproval = null
      }
    } finally { if (request.token === selectionToken) approvalBusy = false }
  }

  /** "知道了" lasts until a newer message; a mute lasts 7 days unless the sender escalates. */
  function dismissSignal(item: RuntimeAttention, mode: 'ack' | 'mute') {
    if (!item.signal || pendingDelegation || delegationBusy) return
    void submitDelegation({ kind: 'signal', data: { ...item.signal, mode, ...(mode === 'mute' ? { until: muteUntil(Date.now()) } : {}) } })
  }

  let delegationControls = $derived<DelegationControls | undefined>(!sample && runtime.delegation ? {
    view: runtime.delegation,
    disabled: (writeBlocked && !pendingDelegation) || awaitingLiveState || projectState?.status !== 'ready',
    busy: delegationBusy, pending: pendingDelegation?.action ?? null, error: delegationError, result: delegationResult, ruleOffer,
    onAction: (intent) => { void submitDelegation(intent) },
    onRetry: () => { void sendDelegation(true) },
    onDismissRule: () => { ruleOffer = null },
  } : undefined)

  async function openTaskCreate(module?: string, idea?: IdeaEntry) {
    if (sample || !project || failedSelection || (writeBlocked && pendingTaskControl?.kind !== 'task')) return
    closeOverlays()
    showTaskCreate = true
    promotingIdea = pendingTaskControl ? null : idea ?? null
    if (pendingTaskControl) return
    taskControlError = null
    taskControlReceipt = null
    taskOptions = null
    const token = project.token, request = ++taskOptionsGeneration
    taskOptionsLoading = true
    try {
      if (!client.taskOptions) throw new Error('当前客户端不支持读取任务范围。')
      const value = await client.taskOptions(token)
      if (token === selectionToken && request === taskOptionsGeneration) {
        taskOptions = { ...value, modules: [...value.modules].sort((a, b) => Number(b.id === module) - Number(a.id === module)) }
      }
    } catch (cause) { if (token === selectionToken && request === taskOptionsGeneration) taskControlError = String(cause) }
    finally { if (token === selectionToken && request === taskOptionsGeneration) taskOptionsLoading = false }
  }

  function openControl() {
    if (sample || !project || failedSelection || (writeBlocked && pendingTaskControl?.kind !== 'control')) return
    closeOverlays()
    if (!pendingTaskControl) { taskControlError = null; taskControlReceipt = null }
    showControl = true
  }

  async function submitTask(task: HumanTaskDraft) {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    pendingTaskControl = { token: project.token, kind: 'task', input: { request_id: humanRequestId(), task } }
    await sendTaskControl(false)
  }

  /** With no project yet, the sentence waits while the person picks the folder it belongs to. */
  async function submitIdea(text: string): Promise<boolean> {
    if (sample || writeBlocked) return false
    ideaError = null
    ideaReceipt = null
    if (!project) {
      await chooseProject()
      if (!project || failedSelection) {
        if (error) ideaError = '这句话还在输入框里，项目打开后再记下。所选文件夹还不是 Tower.md 项目时，先在终端运行 towermd init。'
        return false
      }
    }
    if (failedSelection || !project) return false
    pendingIdea = { token: project.token, kind: 'add', input: { request_id: humanRequestId(), text } }
    return sendIdea(false)
  }

  function promoteIdea(idea: IdeaEntry, as: 'project' | 'task') {
    if (sample || !project || failedSelection || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    ideaError = null
    ideaReceipt = null
    if (as === 'project') {
      pendingIdea = { token: project.token, kind: 'promote', input: { request_id: humanRequestId(), idea: idea.id, as: 'project' } }
      void sendIdea(false)
      return
    }
    // A task needs a scope from the approved version: the same card as a new task, seeded with the sentence.
    void openTaskCreate(undefined, idea)
  }

  /** An unconfirmed promotion is only ever retried with its frozen request. */
  function resumePromotion() {
    if (pendingIdea?.kind !== 'promote' || ideaBusy) return
    if (pendingIdea.input.as === 'project') { void sendIdea(); return }
    closeOverlays()
    showTaskCreate = true
  }

  async function submitPromotion(task: HumanTaskDraft) {
    if (!project || !promotingIdea || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    pendingIdea = { token: project.token, kind: 'promote', input: { request_id: humanRequestId(), idea: promotingIdea.id, as: 'task', task } }
    await sendIdea(false)
  }

  async function sendIdea(retrying = true): Promise<boolean> {
    const request = pendingIdea
    if (!request || sample || ideaBusy || request.token !== selectionToken) return false
    ideaBusy = true
    ideaError = null
    try {
      let receipt: IdeaReceipt
      if (request.kind === 'add') {
        if (!client.addIdea) throw new Error('操作已拒绝：当前客户端不支持记录想法。')
        receipt = await client.addIdea(request.token, request.input)
        if (receipt.idea?.text !== request.input.text) throw new Error('想法回执与原文不一致，请重试原请求。')
      } else {
        if (!client.promoteIdea) throw new Error('操作已拒绝：当前客户端不支持立项。')
        receipt = await client.promoteIdea(request.token, request.input)
        if (receipt.idea?.id !== request.input.idea || receipt.idea.promoted?.as !== request.input.as) throw new Error('立项回执与原请求不一致，请重试原请求。')
      }
      if (request.token !== selectionToken) return false
      if (receipt.request_id !== request.input.request_id || receipt.event !== request.input.request_id
        || receipt.status !== 'accepted' || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0) {
        throw new Error('回执与原请求不一致，请重试原请求确认。')
      }
      if (request.kind === 'add') ideaReceipt = '已记下这个想法；它不进关注清单，也不会派给受托者，立项后才算数。'
      else {
        promotedThisVisit += 1
        promotingIdea = null
        ideaReceipt = request.input.as === 'project' ? '已立为项目目标；由受托者起草第一个版本，经你批准后生效。' : `已立为任务 ${request.input.task.id}`
        if (request.input.as === 'task') { showTaskCreate = false; taskOptions = null; showIdeas = true }
      }
      pendingIdea = null
      refreshLiveState(receipt.seq)
      return true
    } catch (cause) {
      if (request.token === selectionToken) {
        ideaError = String(cause)
        if (!retrying && ideaError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingIdea = null
      }
      return false
    } finally { if (request.token === selectionToken) ideaBusy = false }
  }

  function addFollowUp(text: string, when: FollowUpWhen): Promise<boolean> {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return Promise.resolve(false)
    rhythmError = null
    rhythmReceipt = null
    lastRhythm = 'add'
    pendingRhythm = { token: project.token, kind: 'add', input: { request_id: humanRequestId(), text, when } }
    return sendRhythm(false)
  }
  function finishFollowUp(follow_up: string) {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    rhythmError = null
    rhythmReceipt = null
    lastRhythm = 'done'
    pendingRhythm = { token: project.token, kind: 'done', input: { request_id: humanRequestId(), follow_up } }
    void sendRhythm(false)
  }
  function seeResurfaced(object: string) {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    rhythmError = null
    rhythmReceipt = null
    lastRhythm = 'seen'
    pendingRhythm = { token: project.token, kind: 'seen', input: { request_id: humanRequestId(), object } }
    void sendRhythm(false)
  }

  async function sendRhythm(retrying = true): Promise<boolean> {
    const request = pendingRhythm
    if (!request || sample || rhythmBusy || request.token !== selectionToken) return false
    rhythmBusy = true
    rhythmError = null
    try {
      let receipt: { request_id: string; event: string; seq: number; status: string }
      if (request.kind === 'add') {
        if (!client.addFollowUp) throw new Error('操作已拒绝：当前客户端不支持跟进项。')
        const added = await client.addFollowUp(request.token, request.input)
        if (added.follow_up?.text !== request.input.text) throw new Error('跟进项回执与原文不一致，请重试原请求。')
        receipt = added
      } else if (request.kind === 'done') {
        if (!client.finishFollowUp) throw new Error('操作已拒绝：当前客户端不支持跟进项。')
        const done = await client.finishFollowUp(request.token, request.input)
        if (done.follow_up?.id !== request.input.follow_up || !done.follow_up.done) throw new Error('完成回执与原请求不一致，请重试原请求。')
        receipt = done
      } else {
        if (!client.seeResurfaced) throw new Error('操作已拒绝：当前客户端不支持浮现。')
        const seen = await client.seeResurfaced(request.token, request.input)
        if (seen.object !== request.input.object) throw new Error('回执与原请求不一致，请重试原请求。')
        receipt = seen
      }
      if (request.token !== selectionToken) return false
      if (receipt.request_id !== request.input.request_id || receipt.event !== request.input.request_id
        || receipt.status !== 'accepted' || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0) {
        throw new Error('回执与原请求不一致，请重试原请求确认。')
      }
      rhythmReceipt = request.kind === 'add' ? '已记下跟进项；到点后进关注清单。' : request.kind === 'done' ? '跟进项已完成。' : '已记下看过了。'
      pendingRhythm = null
      refreshLiveState(receipt.seq)
      return true
    } catch (cause) {
      if (request.token === selectionToken) {
        rhythmError = String(cause)
        if (!retrying && rhythmError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingRhythm = null
      }
      return false
    } finally { if (request.token === selectionToken) rhythmBusy = false }
  }

  async function openFrames() {
    const open = !showFrames
    closeOverlays()
    showFrames = open
    if (open && !templates) await loadTemplates()
  }

  async function loadTemplates() {
    if (!project || !client.listTemplates || templatesLoading) return
    const token = project.token
    const request = ++templatesGeneration
    templatesLoading = true
    templatesError = null
    try {
      const listed = await client.listTemplates(token)
      if (request === templatesGeneration && token === selectionToken) templates = listed.templates
    } catch (cause) {
      if (request === templatesGeneration && token === selectionToken) templatesError = `读取模板失败：${String(cause)}`
    } finally { if (request === templatesGeneration) templatesLoading = false }
  }

  function templateOver(event: DragEvent) {
    if (!frameMode || !event.dataTransfer?.types.includes(TEMPLATE_MIME)) return
    event.preventDefault()
    event.dataTransfer.dropEffect = 'copy'
  }

  /** FR-144: a template dropped on the canvas waits in the tray for its name. */
  function templateDropped(event: DragEvent) {
    const id = event.dataTransfer?.getData(TEMPLATE_MIME)
    if (!frameMode || !id) return
    event.preventDefault()
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    droppedTemplate = { id, at: canvas ? canvas.canvasPoint(event.clientX - rect.left, event.clientY - rect.top) : { x: 0, y: 0 } }
    if (!showFrames) void openFrames()
  }

  /** FR-142/144/153: a new frame takes the nearest blank spot; nothing already on the canvas moves. */
  async function placeTemplate(choice: FrameChoice): Promise<boolean> {
    if (!project || !frameMode || writeBlocked || awaitingLiveState || !choice.template.layout) return false
    if (converting) return convertSelection(choice)
    frameError = null
    frameReceipt = null
    const at = droppedTemplate?.id === choice.template.id ? droppedTemplate.at
      : canvas ? canvas.canvasPoint(shellWidth / 2, shellHeight / 2) : { x: 0, y: 0 }
    const taken = [...frames.map((frame) => frame.place),
      ...(view?.nodes ?? []).map((node) => ({ x: node.x, y: node.y, width: node.width, height: node.height }))]
    const input: FrameRequest = { request_id: humanRequestId(), id: newFrameId(frames.map((frame) => frame.id)), title: choice.title,
      template: choice.template.id, set_by: choice.setBy, place: blankSpot(at, defaultSize(choice.template.layout), taken) }
    if (choice.rules) input.rules = { text: choice.rules }
    if (choice.mode) input.mode = choice.mode
    pendingFrame = { token: project.token, what: `放置容器「${choice.title}」`, kind: 'create', input }
    droppedTemplate = null
    return (await sendFrame(false)) !== null
  }

  /** FR-144 方式 2: the canvas picked nodes; a template beside it finishes the frame. */
  function startConvert(nodes: CanvasNode[], edges: CanvasEdge[]) {
    if (!project || !frameMode || !nodes.length) return
    converting = { token: project.token, nodes, edges }
    droppedTemplate = null
    frameError = null
    frameReceipt = null
    showFrames = true
    if (!templates) void loadTemplates()
  }

  /**
   * FR-144 方式 2: one frame beside the picked nodes, then the person's own
   * cards in it, then their first places. Each step is one write; a step
   * that fails waits in the tray to be retried or given up, and the steps
   * already written stay. The picked nodes never move.
   */
  async function convertSelection(choice: FrameChoice): Promise<boolean> {
    const picked = converting
    const layout = choice.template.layout
    if (!project || !picked || picked.token !== project.token || !layout) return false
    frameError = null
    frameReceipt = null
    const plan = conversion(choice.template, picked.nodes, picked.edges)
    const lines = [...plan.copies.flatMap((copy) => (copy.parent ? [{ from: copy.parent, to: copy.node }] : [])), ...plan.links]
    let laid: Awaited<ReturnType<typeof initialLayout>> | null = null
    try {
      if (needsInitialLayout(layout) && plan.copies.length) laid = await initialLayout(plan.copies.map((copy) => copy.node), lines)
    } catch (cause) {
      frameError = `初始布局失败：${String(cause)}`
      return false
    }
    const size = defaultSize(layout)
    if (laid) { size.width = Math.max(size.width, laid.width); size.height = Math.max(size.height, laid.height) }
    const right = Math.max(...picked.nodes.map((node) => node.x + node.width))
    const top = Math.min(...picked.nodes.map((node) => node.y))
    const taken = [...frames.map((frame) => frame.place),
      ...(view?.nodes ?? []).map((node) => ({ x: node.x, y: node.y, width: node.width, height: node.height }))]
    const id = newFrameId(frames.map((frame) => frame.id))
    const input: FrameRequest = { request_id: humanRequestId(), id, title: choice.title, template: choice.template.id, set_by: choice.setBy,
      place: blankSpot({ x: right + 48 + size.width / 2, y: top + size.height / 2 }, size, taken) }
    if (choice.rules) input.rules = { text: choice.rules }
    if (plan.mode !== 'free') { input.mode = plan.mode; input.objects = plan.objects }
    const token = project.token
    const step = async (write: FrameWrite) => { pendingFrame = write; return sendFrame(false) }
    if (!(await step({ token, what: `把 ${picked.nodes.length} 个节点转为容器「${choice.title}」`, kind: 'create', input }))) return false
    converting = null
    const cards = new Map<string, string>()
    for (const copy of plan.copies) {
      const parent = copy.parent ? cards.get(copy.parent) : undefined
      const written = await step({ token, what: `转为容器：加卡片「${copy.title}」`, kind: 'card',
        input: { request_id: humanRequestId(), frame: id, op: { op: 'add', title: copy.title, ...(parent ? { parent } : {}) } } }) as CardReceipt | null
      if (!written?.receipt?.card) { frameError ??= `转为容器停在卡片「${copy.title}」，已写入的部分保留。`; return false }
      cards.set(copy.node, written.receipt.card)
    }
    for (const link of plan.links) {
      const from = cards.get(link.from)
      const to = cards.get(link.to)
      if (from && to && !(await step({ token, what: '转为容器：连线', kind: 'card', input: { request_id: humanRequestId(), frame: id, target: from, op: { op: 'link', to } } }))) return false
    }
    const pins = Object.fromEntries(Object.entries(laid?.pins ?? {}).flatMap(([node, pin]) => (cards.has(node) ? [[cards.get(node)!, pin]] : [])))
    if (Object.keys(pins).length && !(await step({ token, what: '转为容器：初始布局', kind: 'pin', input: { request_id: humanRequestId(), frame: id, pins } }))) return false
    frameReceipt = `已把 ${picked.nodes.length} 个节点转为容器「${choice.title}」；画布上原来的节点没有移动。`
    return true
  }

  /** FR-145: the person moved or resized a frame; it stays where they put it while the daemon confirms. */
  function placeFrame(id: string, place: FramePlace) {
    const frame = frames.find((entry) => entry.id === id)
    if (!project || !frameMode || writeBlocked || awaitingLiveState || !frame || samePlace(frame.place, place)) return
    frameError = null
    frameReceipt = null
    placedLocally = { ...placedLocally, [id]: { place, seq: null } }
    pendingFrame = { token: project.token, what: `移动容器「${frame.title}」`, kind: 'place', input: { request_id: humanRequestId(), frame: id, place } }
    void sendFrame(false)
  }

  /** One write on a frame's content; stable so frame nodes are not rebuilt for it. */
  function writeFrame(write: FrameWrite) {
    if (!project || !frameMode || writeBlocked || awaitingLiveState) return
    frameError = null
    frameReceipt = null
    pendingFrame = write
    void sendFrame(false)
  }
  const frameActions: FrameActions = {
    pin: (frame, pins) => {
      if (!project || !frameMode || writeBlocked || awaitingLiveState) return
      pinnedLocally = { ...pinnedLocally, [frame]: { pins: { ...pinnedLocally[frame]?.pins, ...pins }, seq: null } }
      writeFrame({ token: project.token, what: Object.values(pins).some((pin) => pin) ? '固定卡片位置' : '取消固定卡片',
        kind: 'pin', input: { request_id: humanRequestId(), frame, pins } })
    },
    card: (frame, target, op) => project && writeFrame({ token: project.token, what: op.op === 'add' ? `加卡片「${op.title}」` : `修改卡片 ${target}`,
      kind: 'card', input: { request_id: humanRequestId(), frame, ...(target === frame ? {} : { target }), op } }),
    rules: (frame, rules) => project && writeFrame({ token: project.token, what: `修改容器 ${frame} 的规则`, kind: 'rules', input: { request_id: humanRequestId(), frame, rules } }),
    resolve: (frame, pending, accept) => project && writeFrame({ token: project.token, what: accept ? '接受待处理项' : '忽略待处理项',
      kind: 'resolve', input: { request_id: humanRequestId(), frame, pending, accept } }),
  }

  function abandonFrame() {
    const request = pendingFrame
    if (!request || frameBusy) return
    if (request.kind === 'place') {
      const rest = { ...placedLocally }
      delete rest[request.input.frame]
      placedLocally = rest
    } else if (request.kind === 'pin') {
      const rest = { ...pinnedLocally }
      delete rest[request.input.frame]
      pinnedLocally = rest
    }
    pendingFrame = null
    frameError = null
  }

  /** The daemon's receipt, or `null` when the write did not go through. */
  async function sendFrame(retrying = true): Promise<FrameReceipt | null> {
    const request = pendingFrame
    if (!request || sample || frameBusy || request.token !== selectionToken) return null
    frameBusy = true
    frameError = null
    let rejected = false
    try {
      let receipt: FrameReceipt
      if (request.kind === 'create') {
        if (!client.createFrame) throw new Error('操作已拒绝：当前客户端不支持放置容器。')
        receipt = await client.createFrame(request.token, request.input)
        if (receipt.frame?.id !== request.input.id || receipt.frame.title !== request.input.title) throw new Error('容器回执与原请求不一致，请重试原请求。')
      } else if (request.kind === 'place') {
        if (!client.placeFrame) throw new Error('操作已拒绝：当前客户端不支持移动容器。')
        receipt = await client.placeFrame(request.token, request.input)
        if (receipt.frame?.id !== request.input.frame || !samePlace(receipt.frame.place, request.input.place)) throw new Error('容器回执与原请求不一致，请重试原请求。')
      } else {
        let written: CardReceipt
        if (request.kind === 'card') {
          if (!client.writeCard) throw new Error('操作已拒绝：当前客户端不支持写卡片。')
          written = await client.writeCard(request.token, request.input)
        } else if (request.kind === 'pin') {
          if (!client.pinCards) throw new Error('操作已拒绝：当前客户端不支持固定卡片。')
          written = await client.pinCards(request.token, request.input)
        } else if (request.kind === 'rules') {
          if (!client.changeFrameRules) throw new Error('操作已拒绝：当前客户端不支持修改规则。')
          written = await client.changeFrameRules(request.token, request.input)
        } else {
          if (!client.resolveFrameItem) throw new Error('操作已拒绝：当前客户端不支持处理待办项。')
          written = await client.resolveFrameItem(request.token, request.input)
        }
        if (written.frame?.id !== request.input.frame) throw new Error('容器回执与原请求不一致，请重试原请求。')
        receipt = written
        if (written.receipt?.hint) frameReceipt = written.receipt.hint
      }
      if (request.token !== selectionToken) return null
      if (receipt.request_id !== request.input.request_id || receipt.event !== request.input.request_id
        || receipt.status !== 'accepted' || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0) {
        throw new Error('回执与原请求不一致，请重试原请求确认。')
      }
      if (request.kind === 'place') placedLocally = { ...placedLocally, [request.input.frame]: { place: request.input.place, seq: receipt.seq } }
      else if (request.kind === 'pin') pinnedLocally = { ...pinnedLocally, [request.input.frame]: { pins: pinnedLocally[request.input.frame]?.pins ?? request.input.pins, seq: receipt.seq } }
      else if (request.kind === 'create') frameReceipt = `已放置容器「${request.input.title}」；只有你能移动或缩放它。`
      else flushFrames = true
      pendingFrame = null
      refreshLiveState(receipt.seq)
      return receipt
    } catch (cause) {
      if (request.token === selectionToken) {
        frameError = String(cause)
        rejected = !retrying && frameError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')
      }
      return null
    } finally {
      if (request.token === selectionToken) {
        frameBusy = false
        if (rejected) { const error = frameError; abandonFrame(); frameError = error }
      }
    }
  }

  async function submitControl(pause_event?: string) {
    if (!project || sample || writeBlocked || awaitingLiveState || projectState?.status !== 'ready') return
    pendingTaskControl = { token: project.token, kind: 'control', input: pause_event
      ? { kind: 'resume', request_id: humanRequestId(), pause_event }
      : { kind: 'pause', request_id: humanRequestId() } }
    await sendTaskControl(false)
  }

  async function sendTaskControl(retrying = true) {
    const request = pendingTaskControl
    if (!request || sample || taskControlBusy || request.token !== selectionToken) return
    taskControlBusy = true
    taskControlError = null
    try {
      let receipt: HumanTaskReceipt | ControlReceipt
      if (request.kind === 'task') {
        if (!client.createHumanTask) throw new Error('操作已拒绝：当前客户端不支持创建任务。')
        const taskReceipt = await client.createHumanTask(request.token, request.input)
        receipt = taskReceipt
        if (Object.entries(request.input.task).some(([key, value]) => JSON.stringify(taskReceipt.task.input[key as keyof HumanTaskDraft]) !== JSON.stringify(value))) {
          throw new Error('任务回执与冻结输入不一致，请重试原请求。')
        }
        if (receipt.task.origin !== 'app_human') throw new Error('任务回执来源不一致。')
      } else {
        if (!client.controlProject) throw new Error('操作已拒绝：当前客户端不支持暂停/恢复。')
        receipt = await client.controlProject(request.token, request.input)
      }
      if (request.token !== selectionToken) return
      if (receipt.request_id !== request.input.request_id || receipt.event !== request.input.request_id
        || receipt.status !== 'accepted' || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0) {
        throw new Error('回执与原请求不一致，请重试原请求确认。')
      }
      taskControlReceipt = request.kind === 'task' ? request.input.task.clarify ? `已委托任务 ${request.input.task.id}，等待受托者澄清` : `已委托任务 ${request.input.task.id}`
        : request.input.kind === 'pause' ? '暂停请求已记录；运行中的受托者在检查点读取指令。'
        : '恢复确认已记录；以当前项目状态为准。'
      pendingTaskControl = null
      if (request.kind === 'task') taskOptions = null
      refreshLiveState(receipt.seq)
    } catch (cause) {
      if (request.token === selectionToken) {
        taskControlError = String(cause)
        if (!retrying && taskControlError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingTaskControl = null
      }
    } finally { if (request.token === selectionToken) taskControlBusy = false }
  }

  async function mainResolutionCompleted(seq?: number) {
    if (seq !== undefined) minimumSeq = Math.max(minimumSeq, seq)
    await refreshAuthority(mainResolution?.token)
  }

  async function refreshAuthority(token = project?.token) {
    startLiveUpdates()
    if (!token) return
    try {
      const updated = await client.loadProject(token)
      if (token === selectionToken) view = updated
    } catch (cause) { if (token === selectionToken) stateError = String(cause) }
  }

  function openHuman(context: HumanContext) {
    if (sample || !project || failedSelection || (writeBlocked && !pendingHuman)) return
    if (!pendingHuman && projectState?.status !== 'ready') return
    closeOverlays()
    humanContext = pendingHuman?.context ?? context
    if (!pendingHuman) { humanError = null; humanReceipt = null; humanRule = null }
    showHuman = true
  }

  /**
   * FR-70: plain text is sent as typed; a quick command maps to an action
   * the App already has, through that action's own retry and receipt.
   */
  async function submitHuman(text: string, destination: AnswerDestination) {
    if (sample || !project || !humanContext || pendingHuman || humanBusy || writeBlocked || awaitingLiveState || !text.trim()) return
    const request_id = humanRequestId()
    lastQuick = null
    const context = humanContext
    const command = parseQuickCommand(text, context.kind === 'answer' ? { kind: 'answer' } : { kind: 'prompt', scope: context.scope })
    humanError = null
    switch (command.kind) {
      case 'error': humanError = command.message; return
      case 'pause':
      case 'resume': {
        if ((command.kind === 'pause') === control.paused) { humanError = control.paused ? '项目已经暂停。' : '项目没有暂停，无需恢复。'; return }
        openControl()
        await submitControl(command.kind === 'resume' ? control.pause_event ?? undefined : undefined)
        return
      }
      case 'report':
        openTasks(command.target)
        await submitDelegation({ kind: 'ask', data: { target: command.target, ask: 'report', text: command.text, due: command.due } })
        return
      case 'rule':
        pendingHuman = { token: project.token, context, kind: 'rule',
          input: { request_id, text: command.text, scope: command.scope, until: command.until } }
        break
      case 'answer':
      case 'message':
        if (context.kind === 'answer') {
          if (destination !== 'temporary' && destination !== 'module_decision') return
          pendingHuman = { token: project.token, context, kind: 'answer',
            input: { request_id, hold: context.question.draft.id, text: command.text, remember_as: destination, targets: [],
              ...(command.kind === 'answer' ? { assumption: command.assumption } : {}) } }
        } else {
          pendingHuman = { token: project.token, context, kind: 'prompt',
            input: { request_id, scope: context.scope, text: command.text, targets: [] } }
        }
    }
    await sendHuman(false)
  }

  function frozenHuman(request: HumanRequest): { text: string; destination: AnswerDestination } {
    if (request.kind === 'rule') return { text: `临时规则：${request.input.text}\n到期：${request.input.until}`, destination: 'temporary' }
    if (request.kind === 'prompt') return { text: request.input.text, destination: 'temporary' }
    const assumed = request.input.assumption ? `\n（假设：${request.input.assumption}）` : ''
    return { text: request.input.text + assumed, destination: request.input.remember_as }
  }

  async function sendHuman(retrying = true) {
    const request = pendingHuman
    if (!request || humanBusy || sample || request.token !== selectionToken) return
    humanBusy = true
    humanError = null
    try {
      let receipt: HumanReceipt
      let rule: TempRuleEntry | null = null
      if (request.kind === 'answer') {
        if (!client.answerQuestion) throw new Error('操作已拒绝：当前客户端不支持回答问题。')
        receipt = await client.answerQuestion(request.token, request.input)
      } else if (request.kind === 'rule') {
        if (!client.addTempRule) throw new Error('操作已拒绝：当前客户端不支持临时规则。')
        const added = await client.addTempRule(request.token, request.input)
        rule = added.rule
        if (rule?.id !== `TR-${request.input.request_id}` || rule.text !== request.input.text || rule.until !== request.input.until
          || JSON.stringify(rule.scope) !== JSON.stringify(request.input.scope)) {
          throw new Error('临时规则回执与原请求不一致，请重试原提交确认。')
        }
        receipt = { request_id: added.request_id, event: added.event, seq: added.seq, status: added.status, proposal: null }
      } else {
        if (!client.sendPrompt) throw new Error('操作已拒绝：当前客户端不支持插话。')
        receipt = await client.sendPrompt(request.token, request.input)
      }
      if (request.token !== selectionToken) return
      if (receipt.request_id !== request.input.request_id || receipt.status !== 'accepted'
        || receipt.event !== request.input.request_id || !Number.isSafeInteger(receipt.seq) || receipt.seq <= 0
        || !(receipt.proposal === null || typeof receipt.proposal === 'string')) {
        throw new Error('人工提交回执与原请求不一致，请重试原提交确认。')
      }
      humanReceipt = receipt
      humanRule = rule
      pendingHuman = null
      refreshLiveState(receipt.seq)
    } catch (cause) {
      if (request.token === selectionToken) {
        humanError = String(cause)
        if (!retrying && humanError.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) pendingHuman = null
      }
    } finally { if (request.token === selectionToken) humanBusy = false }
  }

  async function requestRecovery() {
    if (sample || !project || writeBlocked || busy || projectState?.status !== 'needs_recovery') return
    const token = project.token
    recoveryBusy = true
    recoveryError = null
    try {
      if (!client.requestRecovery) throw new Error('当前客户端不支持恢复请求。')
      const result = await client.requestRecovery(token)
      if (token === selectionToken) recovery = { token, id: result.request_id }
    } catch (cause) { if (token === selectionToken) recoveryError = String(cause) }
    finally { if (token === selectionToken) recoveryBusy = false }
  }

  async function confirmRecovery() {
    const request = recovery
    if (sample || !request || request.token !== selectionToken || recoveryBusy || interactionWriteBlocked
      || publicationBusy || publicationAttempted || pendingEdit || editBusy || pendingVerification || verificationBusy) return
    recoveryAttempted = true
    recoveryBusy = true
    recoveryError = null
    try {
      if (!client.confirmRecovery) throw new Error('当前客户端不支持人工确认恢复。')
      const result = await client.confirmRecovery(request.token, request.id)
      if (request.token !== selectionToken) return
      if (result.status !== 'ready' || !result.snapshot_commit) throw new Error('恢复回执不完整，请重试原确认。')
      minimumSeq = Math.max(minimumSeq, result.seq)
      startLiveUpdates()
      recoveryCommit = result.snapshot_commit
      recovery = null
      recoveryAttempted = false
    } catch (cause) { if (request.token === selectionToken) recoveryError = String(cause) }
    finally { if (request.token === selectionToken) recoveryBusy = false }
  }

  function moveNode(id: string, x: number, y: number) {
    const node = draft?.view.nodes.find((entry) => entry.id === id)
    if (!node || (node.x === x && node.y === y)) return
    void saveEdit({ kind: 'move', node_id: id, x: Math.round(x), y: Math.round(y) })
  }

  function openReference(path = '', point?: { x: number; y: number }) {
    if (sample || !draft || draft.stale || writeBlocked) return
    closeOverlays()
    referencePath = path
    referencePoint = point ? { x: Math.round(point.x), y: Math.round(point.y) } : { x: 0, y: Math.max(0, ...draft.view.nodes.map((node) => node.y + node.height)) + 80 }
    showReference = true
  }

  // Closing the window counts as leaving the project; switching projects is recorded in `load`.
  function leaveProject() {
    if (project && !failedSelection) recordVisit(project.path, Date.now())
  }
  onMount(() => {
    window.addEventListener('pagehide', leaveProject)
    return () => { window.removeEventListener('pagehide', leaveProject); leaveProject() }
  })

  onMount(() => {
    if (!isTauri()) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void getCurrentWebview().onDragDropEvent(async ({ payload }) => {
      if (sample || payload.type !== 'drop' || !project || !draft || writeBlocked || draft.stale || busy) return
      const token = project.token
      try {
        if (payload.paths.length !== 1) throw new Error('请每次拖入一个 Markdown 文件。')
        if (!client.referencePath) throw new Error('当前客户端不支持文件拖入。')
        const path = await client.referencePath(token, payload.paths[0])
        if (token !== selectionToken || disposed || writeBlocked) return
        const ratio = window.devicePixelRatio || 1
        openReference(path, canvas?.canvasPoint(payload.position.x / ratio, payload.position.y / ratio))
      } catch (cause) { if (token === selectionToken && !disposed) editError = String(cause) }
    }).then((stop) => { if (disposed) stop(); else unlisten = stop }).catch((cause) => {
      if (!disposed) editError = `文件拖入不可用，可从画布菜单输入引用路径：${String(cause)}`
    })
    return () => { disposed = true; unlisten?.() }
  })

  function openSample() {
    if (writeBlocked || busy || startPending) return
    stopPolling()
    stopLiveUpdates()
    closeOverlays()
    sample = createSample()
    sampleError = null
    fitVersion += 1
  }

  function closeSample() {
    closeOverlays()
    sample = null
    sampleError = null
    fitVersion += 1
    startLiveUpdates()
  }

  function sampleAction(action: SampleAction) {
    if (!sample) return
    try {
      sample = applySampleAction(sample, action)
      if (selected) selected = sampleView(sample).nodes.find((node) => node.id === selected?.id) ?? null
      sampleError = null
    } catch (cause) { sampleError = String(cause) }
  }

  function contextMenu(event: MouseEvent) {
    event.preventDefault()
    menu = {
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - 224)),
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 490)),
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') closeOverlays()
    if (event.metaKey || event.ctrlKey) {
      if (event.key.toLowerCase() === 'o') {
        event.preventDefault()
        void chooseProject()
      } else if (event.key.toLowerCase() === 'r') {
        event.preventDefault()
        void refresh()
      }
    }
    if (event.shiftKey && event.key === 'F10') {
      event.preventDefault()
      menu = { x: 24, y: 24 }
    }
  }

  function openNode(node: CanvasNode) {
    if (node.kind === 'group' && node.region === 'project' && !sample && !candidate && !draft) {
      openTasks()
      return
    }
    closeOverlays()
    selected = node
  }
</script>

<svelte:window onkeydown={keydown} />

<main class="canvas-shell" class:is-candidate={!!sample || candidate || !!draft} aria-label="Tower.md 画布" oncontextmenu={contextMenu}
  bind:clientWidth={shellWidth} bind:clientHeight={shellHeight} ondragover={templateOver} ondrop={templateDropped}>
  {#if displayed && (displayed.nodes.length > 0 || frames.length > 0)}
    <ProjectCanvas bind:this={canvas} view={displayed} {statuses} {activity} attention={sample || candidate || draft ? undefined : runtime.allAttention} budget={runtime.shown} rhythm={sample || candidate || draft ? {} : rhythmMarks(runtime.rhythm)} backlog={sample || candidate || draft ? undefined : runtime.backlog?.count} {fitVersion} editable={!sample && !!draft && !draft.stale && !writeBlocked && !busy} onMove={moveNode} onOpen={openNode}
      frames={shownFrames} frameBadges={badges} framesEditable={frameMode && !writeBlocked && !awaitingLiveState && !busy} onPlaceFrame={placeFrame} {frameActions}
      since={visit?.since} onConvert={startConvert} />
  {:else}
    <div class="canvas-empty">
      {#if view}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>{view.title}</h1>
        <p>{draft ? '此综合草稿没有可显示的节点。' : candidate ? '本次扫描没有可显示的候选节点。' : '当前提交还没有可显示的画布节点。'}</p>
        {#if view.docs_dir}
          <button class="primary-button" onclick={startScan} disabled={busy || scanning || startPending || jobActive || !!failedSelection}>扫描并验证</button>
        {/if}
        <button class="subtle-button" onclick={openProjectState}>{view.unplaced.length ? `查看未摆放模块（${view.unplaced.length}）` : '查看项目态势'}</button>
        <button class="subtle-button" onclick={listDrafts} disabled={busy || !!failedSelection}>查看综合草稿</button>
        {#if !sample && !candidate && !draft}
          <div class="canvas-empty-ideas">
            <IdeaInput onSubmit={submitIdea} frozen={pendingIdea?.kind === 'add' ? pendingIdea.input.text : null}
              busy={ideaBusy || busy} error={!promotingIdea && pendingIdea?.kind !== 'promote' ? ideaError : null} onRetry={() => { void sendIdea() }} />
            {#if ideaReceipt}<p class="idea-input-note" role="status">{ideaReceipt}</p>{/if}
            {#if ideas.length}<IdeaCards {ideas} onPromote={promoteIdea} busy={writeBlocked || awaitingLiveState} {hasGoal} hint={ideaHint}
              pending={pendingIdea?.kind === 'promote' ? { error: ideaError, sending: ideaBusy } : null} onResume={resumePromotion} />{/if}
          </div>
        {/if}
      {:else}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>Tower.md</h1>
        <div class="canvas-empty-ideas">
          <IdeaInput onSubmit={submitIdea} busy={busy} error={ideaError} onRetry={() => { void sendIdea() }} />
        </div>
        <p class="canvas-empty-alternative">或者从已有材料开始：</p>
        <button class="subtle-button" onclick={chooseProject} disabled={busy}>打开项目</button>
        <span class="canvas-empty-hint">⌘ / Ctrl + O · <button class="canvas-empty-link" onclick={openSample} disabled={busy || writeBlocked}>打开功能示例</button></span>
      {/if}
    </div>
  {/if}

  {#if frameMode && view}
    <!-- FR-142/144: templates wait beside the canvas; placing one moves nothing already there. -->
    <section class="frame-tray" aria-label="容器模板">
      <button class="idea-tray-toggle" aria-expanded={showFrames} onclick={() => { void openFrames() }}>
        <span aria-hidden="true">▭</span> 容器{frames.length ? ` · ${frames.length} 个` : ''}{pendingFrame && !frameBusy ? ' · 有未确认的操作' : ''}
      </button>
      {#if showFrames || (pendingFrame && !frameBusy)}
        <FrameTray {templates} loading={templatesLoading} loadError={templatesError} busy={writeBlocked || awaitingLiveState || busy}
          pending={pendingFrame ? { what: pendingFrame.what, error: frameError, sending: frameBusy } : null} receipt={frameReceipt}
          dropped={droppedTemplate?.id ?? null} converting={converting?.nodes.length ?? null} onCancelConvert={() => { converting = null }}
          onCreate={placeTemplate} onRetry={() => { void sendFrame() }} onAbandon={abandonFrame}
          onReload={() => { void loadTemplates() }} />
      {/if}
    </section>
  {/if}

  {#if !sample && !candidate && !draft && project && view && displayed && (displayed.nodes.length > 0 || frames.length > 0)}
    <!-- FR-183: ideas float beside the canvas; they add no node and move none. -->
    <section class="idea-tray" aria-label="想法">
      <button class="idea-tray-toggle" aria-expanded={showIdeas} onclick={() => { const open = !showIdeas; closeOverlays(); showIdeas = open }}>
        <span aria-hidden="true">◇</span> 想法{ideas.some((idea) => !idea.promoted) ? ` · ${ideas.filter((idea) => !idea.promoted).length} 条未立项` : ''}
      </button>
      {#if showIdeas}
        <div class="idea-tray-body">
          <IdeaInput label="记下一句话" onSubmit={submitIdea} frozen={pendingIdea?.kind === 'add' ? pendingIdea.input.text : null}
            busy={ideaBusy || busy} error={!promotingIdea && pendingIdea?.kind !== 'promote' ? ideaError : null} onRetry={() => { void sendIdea() }} />
          {#if ideaReceipt}<p class="idea-input-note" role="status">{ideaReceipt}</p>{/if}
          <IdeaCards {ideas} onPromote={promoteIdea} busy={writeBlocked || awaitingLiveState} {hasGoal} hint={ideaHint}
            pending={pendingIdea?.kind === 'promote' ? { error: ideaError, sending: ideaBusy } : null} onResume={resumePromotion} />
        </div>
      {/if}
    </section>
  {/if}

  {#if sample}
    <button class="candidate-badge" onclick={() => { closeOverlays(); selected = sampleView(sample!).nodes.find((node) => node.id === SAMPLE_NODE_IDS.baseline) ?? null }}>功能示例 · 示例数据</button>
  {:else if draft}
    <button class="candidate-badge" onclick={() => { closeOverlays(); showDraft = true }}>综合草稿 · 未批准{draft.stale ? ' · 已过期' : ''}{baselineEvidenceError(draft) ? ' · 证据不可复核' : ''}</button>
  {:else if candidate}
    <button class="candidate-badge" onclick={() => { closeOverlays(); showScan = true }}>候选 · {report?.verification ? '验证报告' : '尚未验证'}</button>
  {:else if digestOpen && digest && view}
    <VisitDigest heading={digest.heading} lines={digest.lines} details={digest.details}
      onOpen={() => { digestOpen = false; openTasks() }} onClose={() => { digestOpen = false }} />
  {/if}

  {#if editBusy}
    <div class="canvas-notice" role="status">正在保存草稿更改…</div>
  {:else if scanning}
    <div class="canvas-notice" role="status">{cancelling ? '正在取消并清理验证进程…' : activeJob?.phase === 'queued' ? '等待验证执行…' : activeJob?.phase === 'verifying' ? '正在验证固定提交…' : '正在分析已提交源码…'}</div>
  {:else if busy}
    <div class="canvas-notice" role="status">正在读取项目…</div>
  {/if}

  {#if editError && !sample}
    <section class="canvas-error" role="alert" aria-label="草稿更改未完成">
      <strong>{pendingEdit ? '保存结果尚未确认' : '无法保存草稿更改'}</strong>
      <p>{editError}</p>
      {#if pendingEdit}<p>原操作已保留。请重试同一操作，确认结果后再继续编辑或切换项目。</p><button onclick={() => saveEdit()} disabled={editBusy}>重试原操作</button>
      {:else}<button onclick={() => { editError = null }}>返回编辑</button>{/if}
    </section>
  {/if}

  {#if error && !sample}
    <section class="canvas-error" role="alert" aria-label="项目读取失败">
      <strong>项目读取失败</strong>
      <p>{error}</p>
      <div class="button-row">
        <button onclick={retry} disabled={busy}>重试</button>
        <button onclick={chooseProject} disabled={busy}>选择其他项目</button>
        {#if view}<button onclick={() => { error = null }}>关闭</button>{/if}
      </div>
    </section>
  {/if}

  {#if scanError && !sample}
    <section class="canvas-error" role="alert" aria-label="仓库扫描失败">
      <strong>仓库扫描失败</strong>
      <p>{scanError}</p>
      <div class="button-row">
        <button onclick={jobActive ? resumeScan : startScan} disabled={busy || scanning || !!failedSelection}>{jobActive ? '重新连接验证' : '重新扫描'}</button>
        <button onclick={() => { scanError = null }}>关闭</button>
      </div>
    </section>
  {/if}

  {#if menu}
    <div class="canvas-menu" role="menu" aria-label="画布菜单" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
      {#if sample}
        <button role="menuitem" onclick={closeSample}>{project ? '关闭示例，返回项目' : '关闭功能示例'}</button>
        <button role="menuitem" onclick={openSample}>重新开始示例</button>
        <button role="menuitem" onclick={() => { fitVersion += 1; menu = null }}>适应画布</button>
      {:else}
      <button role="menuitem" onclick={openSample} disabled={writeBlocked || busy || startPending}>打开功能示例</button>
      <button role="menuitem" onclick={chooseProject} disabled={writeBlocked}>{project ? '切换项目' : '打开项目'} <span>⌘ O</span></button>
      <button role="menuitem" onclick={refresh} disabled={!project || writeBlocked}>刷新 <span>⌘ R</span></button>
      <button role="menuitem" onclick={() => { fitVersion += 1; menu = null }} disabled={!displayed?.nodes.length}>适应画布</button>
      <button role="menuitem" onclick={startScan} disabled={writeBlocked || !view?.docs_dir || busy || scanning || startPending || jobActive || !!failedSelection}>扫描并验证</button>
      <button role="menuitem" onclick={startBaselineScan} disabled={writeBlocked || !view?.docs_dir || busy || scanning || startPending || jobActive || !!failedSelection}>从基线进入草稿</button>
      <button role="menuitem" onclick={() => { closeOverlays(); showExecution = true }}>扫描与执行说明</button>
      <button role="menuitem" onclick={listDrafts} disabled={!project || busy || !!failedSelection || writeBlocked}>查看综合草稿</button>
      {#if draft}
        <button role="menuitem" onclick={() => selectCandidate(false)} disabled={writeBlocked}>返回当前画布</button>
        <button role="menuitem" onclick={() => openReference()} disabled={writeBlocked || draft.stale}>引用 Markdown 文档</button>
        <button role="menuitem" onclick={() => { closeOverlays(); showDraft = true }}>综合说明</button>
        {#if draft.scan.origin}<button role="menuitem" onclick={() => { void openComparison() }}>与 {draft.scan.origin.baseline} 对比</button>{/if}
      {/if}
      {#if jobActive}
        {#if !scanning}<button role="menuitem" onclick={resumeScan}>查看验证进度</button>{/if}
        <button role="menuitem" onclick={() => { menu = null; void cancelScan() }} disabled={cancelling || busy}>取消本次验证</button>
      {/if}
      {#if report}
        <button role="menuitem" onclick={() => selectCandidate(!candidate)}>{candidate ? '返回当前画布' : '查看候选'}</button>
        <button role="menuitem" onclick={() => { closeOverlays(); showScan = true }}>扫描说明</button>
      {/if}
      <button role="menuitem" onclick={openProjectState} disabled={!view || !!failedSelection}>项目态势</button>
      <button role="menuitem" onclick={() => openTasks()} disabled={!view || !!failedSelection}>任务与运行</button>
      <button role="menuitem" onclick={() => { void openTaskCreate() }} disabled={!view || !!failedSelection || (writeBlocked && pendingTaskControl?.kind !== 'task') || projectState?.status !== 'ready'}>{pendingTaskControl?.kind === 'task' ? '继续确认委托' : '新建委托'}</button>
      <button role="menuitem" onclick={openLedger} disabled={!view || !!failedSelection}>信任账本</button>
      {#if pendingDelegation}
        <button role="menuitem" onclick={openPendingDelegation}>继续确认托付操作</button>
      {/if}
      <button role="menuitem" onclick={openControl} disabled={!view || !!failedSelection || (writeBlocked && pendingTaskControl?.kind !== 'control') || projectState?.status !== 'ready'}>{pendingTaskControl?.kind === 'control' ? '继续确认暂停或恢复' : control.paused ? '确认恢复领取' : '暂停全部'}</button>
      <button role="menuitem" onclick={openProposals} disabled={!view || !!failedSelection || (writeBlocked && !proposalBlocked)}>{proposalBlocked ? '继续确认提案' : '提案'}</button>
      {#if pendingHuman}
        <button role="menuitem" onclick={() => openHuman(pendingHuman!.context)}>继续确认人工提交</button>
      {:else}
        <button role="menuitem" onclick={() => openHuman({ kind: 'prompt', scope: { kind: 'global' } })} disabled={writeBlocked || projectState?.status !== 'ready'}>向项目插话</button>
      {/if}
      {#if mainResolutionBlocked && mainResolution}
        <button role="menuitem" onclick={() => openMainResolution(mainResolution!.commit)}>继续确认主干变更</button>
      {/if}
      {/if}
      <button role="menuitem" onclick={() => { menu = null }}>关闭菜单</button>
    </div>
  {/if}

  {#if sample && selected}
    <aside class="canvas-panel" aria-label="示例组件">
      <div class="panel-heading"><h2>{selected.title}</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if sampleError}<p role="alert">{sampleError}</p>{/if}
      {#key selected.id}<SampleDetails state={sample} node={selected} onAction={sampleAction} />{/key}
      <div class="source-footnote"><span>仅供功能体验 · 关闭后清空示例状态</span><button onclick={closeSample}>{project ? '关闭示例，返回项目' : '关闭功能示例'}</button></div>
    </aside>
  {/if}

  {#if showReference && draft}
    <aside class="canvas-panel" aria-label="引用 Markdown 文档">
      <div class="panel-heading"><h2>引用 Markdown 文档</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <p class="panel-description">引用当前项目固定来源提交中的 Markdown 原文。也可以从 Finder 拖入文件；工作区未提交的文档需先提交再重新扫描。</p>
      <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); void saveEdit({ kind: 'reference', path: referencePath, ...referencePoint }) }}>
        <label>项目相对路径<input bind:value={referencePath} placeholder="docs/design.md" required disabled={writeBlocked} /></label>
        <button type="submit" disabled={writeBlocked}>保存引用</button>
      </form>
    </aside>
  {/if}

  {#if showDrafts}
    <aside class="canvas-panel" aria-label="综合草稿">
      <div class="panel-heading"><h2>综合草稿</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <p class="panel-description">受托者使用 towermd dev draft submit 提交后，可在这里查看综合结构。草稿不会自动批准或执行命令。</p>
      {#if draftLoading}<p role="status">正在读取综合草稿…</p>{/if}
      {#if draftError}<p role="alert">{draftError}</p><button onclick={listDrafts}>重新读取草稿</button>{/if}
      {#if !draftLoading && !draftError && !drafts.length}<p class="panel-description">当前项目还没有综合草稿。</p>{/if}
      <div class="draft-list">
        {#each drafts as item}
          <button onclick={() => loadDraft(item.id)} disabled={draftLoading}>
            <strong>{item.title}</strong>
            <span>{item.producer} · {item.created_at}</span>
            <span>{item.stale ? '已过期 · 只供查看' : '未批准'}</span>
            <code>{item.source_revision}</code>
          </button>
        {/each}
      </div>
    </aside>
  {/if}

  {#if showDraft && draft}
    <aside class="canvas-panel" aria-label="综合说明">
      <div class="panel-heading"><h2>综合草稿</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if draft.review}
        <p class="panel-description">重新验证将使用当前审阅后的模块与命令，在固定提交的分离工作树中真实执行；继承环境、允许网络，完成后保留未批准草稿。</p>
        <button onclick={verifyDraft} disabled={interactionWriteBlocked || verificationBusy || editBusy || !!pendingEdit || publicationBusy || publicationAttempted || recoveryBusy || recoveryAttempted || draft.stale || jobActive || (!!pendingVerification && !verificationError)}>{pendingVerification ? '重试原验证请求' : '重新验证当前草稿'}</button>
        {#if verificationJob?.status === 'running'}<p role="status">{verificationJob.cancel_requested ? '正在取消并清理…' : verificationJob.phase === 'queued' ? '等待验证执行…' : '正在验证当前草稿…'}</p><button onclick={cancelVerification} disabled={verificationBusy || verificationJob.cancel_requested}>取消审阅验证</button>{/if}
        {#if verificationJob?.status === 'cancelled'}<p class="panel-description">本次审阅验证已取消；请检查各模块结果。</p>{/if}
        {#if verificationError}<p role="alert">{verificationError}</p>{/if}
        {#if pendingVerification && verificationError}<p class="panel-description">请求结果尚待确认；重试会沿用原请求号，不重复执行命令。</p>{/if}
      {/if}
      <h3>出版版本</h3>
      {#if draft.scan.origin}
        <!-- J4: a later baseline is published only through a proposal a person approves. -->
        <p class="panel-description">本草稿从 {draft.scan.origin.baseline} 进入。后续基线须经提案：由受托者在终端用 towermd propose 提交，再由你在“提案”中审阅并批准。</p>
        <div class="button-row">
          <button onclick={() => { void openComparison() }}>与 {draft.scan.origin.baseline} 对比</button>
          <button onclick={openProposals} disabled={!!failedSelection || (writeBlocked && !proposalBlocked)}>打开提案</button>
        </div>
      {:else if publication}
        <p class="panel-description">批准后会把本次审阅内容提交到主干并创建版本标签。</p>
        <button class="primary-button" onclick={approvePublication} disabled={interactionWriteBlocked || publicationBusy || editBusy || !!pendingEdit || verificationBusy || !!pendingVerification || recoveryBusy || recoveryAttempted}>{publicationBusy ? '正在确认出版…' : publicationAttempted ? `重试原批准 ${publication.preview.target}` : `批准 ${publication.preview.target}`}</button>
        {#if !publicationAttempted}<button onclick={() => { publication = null; publicationError = null }} disabled={publicationBusy}>取消批准预览</button>{/if}
      {:else}
        <button onclick={preparePublication} disabled={writeBlocked || draft.stale || busy || jobActive}>{publicationBusy ? '正在准备…' : '准备批准'}</button>
      {/if}
      {#if publicationError}<p role="alert">{publicationError}</p>{/if}
      {#if publicationAttempted && publicationError}<p class="panel-description">出版结果尚未确认。原批准内容已保留；请重试同一请求，确认结果后再继续编辑或切换项目。</p>{/if}
      <BaselineDetails {draft} publication={publication?.preview ?? null} />
    </aside>
  {/if}

  {#if showCompare && draft}
    <DraftComparisonPanel {comparison} loading={comparisonLoading} error={comparisonError} titleOf={draftTitle}
      onRetry={() => { void openComparison() }} onClose={closeOverlays} onProposals={openProposals} />
  {/if}

  {#if showExecution}
    <aside class="canvas-panel" aria-label="扫描与执行说明">
      <div class="panel-heading"><h2>扫描并验证</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <p class="panel-description">扫描会在固定提交的分离工作树中执行仓库验证命令，继承启动服务时的环境，允许网络，不自动安装依赖。分离工作树不是安全沙箱，命令使用当前用户权限。</p>
      <p class="panel-description">刷新、切换项目或关闭窗口只停止界面跟踪，不会取消服务中的验证任务。需要中止时，使用“取消本次验证”，并等待进程和工作树清理结果。</p>
      <p class="panel-description">验证结果属于候选报告，不代表批准结构或认证任务完成。</p>
    </aside>
  {/if}

  {#if showScan && report}
    <aside class="canvas-panel" aria-label="扫描说明">
      <div class="panel-heading">
        <h2>扫描候选</h2>
        <button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button>
      </div>
      <p class="panel-description">这是根据已提交源码生成的候选结构，未批准，也未写入当前画布。验证结果以实际报告为准。候选固定于下方来源提交；刷新当前画布不会重新扫描。</p>
      {#if report.origin}
        <h3>从 {report.origin.baseline} 进入草稿</h3>
        <p class="panel-description">沿用已批准的模块边界、验证命令与布局，没有重新聚类。生成的草稿未审阅、未批准。</p>
        <button onclick={createBaselineDraft} disabled={busy || writeBlocked || baselineDraftBusy || jobActive || !!failedSelection || !report.verification}>{baselineDraftBusy ? '正在生成草稿…' : '生成草稿并对比'}</button>
        {#if baselineDraftError}<p role="alert">{baselineDraftError}</p>{/if}
      {/if}
      <dl class="source-list">
        <dt>覆盖率</dt><dd>{Math.round(report.coverage * 100)}% · {report.assigned_file_count} / {report.source_file_count} 个源码文件</dd>
        <dt>模块</dt><dd>{report.modules.length} 个候选模块</dd>
        {#if report.checks}
          <dt>8.2 复检</dt><dd>源码分配 {report.checks.coverage_ok ? '通过' : '未通过'} · 模块数 {report.checks.module_count_ok ? '通过' : '未通过'} · 验证命令 {report.checks.commands_ok ? '通过' : '未通过'}</dd>
          <dt>import 证据</dt><dd>{report.checks.imports.connected_files} / {report.checks.imports.supported_files} 个可解析文件（{Math.round(report.checks.import_coverage * 100)}%）</dd>
        {/if}
      </dl>
      {#if report.checks?.low_confidence.length}
        <h3>低置信度模块（虚线显示）</h3>
        <ul class="diagnostics">{#each report.modules as module}{#if report.checks.low_confidence.includes(module.id)}<li>{module.title} · 置信度 {Math.round(module.confidence * 100)}% · {module.rationale}</li>{/if}{/each}</ul>
      {/if}
      {#if report.dependencies?.length}
        <h3>import 依赖</h3>
        <p class="panel-description">依赖线来自源码 import，表示模块间引用，不等于已批准的接口。</p>
        <ul class="diagnostics">{#each report.dependencies as dependency}<li>{report.modules.find((module) => module.id === dependency.from)?.title ?? dependency.from} → {report.modules.find((module) => module.id === dependency.to)?.title ?? dependency.to} · {dependency.imports} 处</li>{/each}</ul>
      {/if}
      <p class="panel-description">扫描并验证会执行仓库命令，继承启动服务时的环境并允许网络，不自动安装依赖；分离工作树不是安全沙箱。</p>
      {#if jobActive}
        <div class="button-row">
          {#if !scanning}<button onclick={resumeScan}>查看验证进度</button>{/if}
          <button onclick={cancelScan} disabled={cancelling || busy}>{cancelling ? '等待取消与清理…' : '取消本次验证'}</button>
        </div>
      {/if}
      {#if activeJob?.status === 'cancelled'}<p class="panel-description">⊘ 本次验证已取消；以各命令结果与清理状态为准。</p>{/if}
      <VerificationDetails {report} running={jobActive && activeJob?.id === report.id} />
      {#if report.modules.some((module) => module.suggested_task)}
        <h3>未发现验证命令的模块建议</h3>
        <ul class="diagnostics">{#each report.modules as module}{#if module.suggested_task}<li>{module.title}：{module.suggested_task}</li>{/if}{/each}</ul>
      {/if}
      {#if report.unassigned.length}
        <h3>未归属源码</h3>
        <ul class="diagnostics">{#each report.unassigned as file}<li>{file}</li>{/each}</ul>
      {/if}
      <h3>历史热点与共同修改</h3>
      <p class="panel-description">热点统计文件出现在历史提交中的次数；共同修改表示两个模块出现在同一历史提交中，不代表调用依赖。</p>
      {#if report.hotspots.length}<ul class="diagnostics">{#each report.hotspots as hotspot}<li>{hotspot.path} · {hotspot.commits} 次提交</li>{/each}</ul>{/if}
      {#if report.couplings.length}<ul class="diagnostics">{#each report.couplings as coupling}<li>{coupling.from} ↔ {coupling.to} · {coupling.commits} 次共同提交</li>{/each}</ul>{/if}
      {#if !report.hotspots.length && !report.couplings.length}<p class="panel-description">没有可显示的历史统计。</p>{/if}
      {#if report.diagnostics.length}<h3>扫描说明</h3><ul class="diagnostics">{#each report.diagnostics as diagnostic}<li>{diagnostic}</li>{/each}</ul>{/if}
      <div class="source-footnote"><span>来源 · {report.reference}</span><code>{report.revision}</code><span>源码树</span><code>{report.tree}</code></div>
    </aside>
  {/if}

  {#if !sample && (showTaskCreate || showControl) && project}
    <aside class="canvas-panel" aria-label={showTaskCreate ? promotingIdea ? '立为任务' : '委托卡' : '暂停与恢复'}>
      <div class="panel-heading"><h2>{showTaskCreate ? promotingIdea ? '立为任务' : '委托卡' : '暂停与恢复'}</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if taskControlReceipt}<p role="status">{taskControlReceipt}</p>{/if}
      {#if awaitingLiveState}<p role="status">操作已记录，正在同步最新状态…</p>{/if}
      {#if showTaskCreate}
        {#if taskControlReceipt}<button onclick={() => openTasks()}>查看任务与运行</button>{/if}
        {#if taskOptionsLoading}<p role="status">正在读取已批准的模块范围…</p>{/if}
        {#if promotingIdea && (taskOptions || pendingIdea?.kind === 'promote')}
          <p class="panel-description">想法：{promotingIdea.text}</p>
          <p class="panel-description">当前批准的版本：{taskOptions?.baseline ?? '原请求记录'}</p>
          {#key promotingIdea.id}
            <TaskCreate modules={taskOptions?.modules ?? []} onSubmit={submitPromotion} seed={promotingIdea.text}
              frozen={pendingIdea?.kind === 'promote' && pendingIdea.input.as === 'task' ? pendingIdea.input.task : null}
              busy={ideaBusy || awaitingLiveState} error={ideaError} backlog={runtime.backlog} onRetry={() => { void sendIdea() }} />
          {/key}
        {:else if taskOptions || pendingTaskControl?.kind === 'task'}
          <p class="panel-description">当前批准的版本：{taskOptions?.baseline ?? '原请求记录'}</p>
          <TaskCreate modules={taskOptions?.modules ?? []} onSubmit={submitTask}
            frozen={pendingTaskControl?.kind === 'task' ? pendingTaskControl.input.task : null}
            busy={taskControlBusy || awaitingLiveState} error={taskControlError} backlog={runtime.backlog} onRetry={() => { void sendTaskControl() }} />
        {:else if taskControlError}<p role="alert">{taskControlError}</p><button onclick={() => { void openTaskCreate(undefined, promotingIdea ?? undefined) }}>重新读取模块</button>{/if}
      {:else}
        <ProjectControl {control} onPause={() => { void submitControl() }} onResume={(id) => { void submitControl(id) }}
          frozen={pendingTaskControl?.kind === 'control' ? pendingTaskControl.input : null}
          busy={taskControlBusy || awaitingLiveState} error={taskControlError} onRetry={() => { void sendTaskControl() }} />
      {/if}
    </aside>
  {/if}

  {#if !sample && showTasks && view}
    <aside class="canvas-panel" aria-label="任务与运行">
      <div class="panel-heading"><h2>任务与运行</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if stateError || connectionError}<p role="alert">状态暂未更新：{stateError || connectionError}</p>{/if}
      {#if projectState?.status === 'ready'}
        <AttentionList items={runtime.allAttention} budget={runtime.shown} asleep={runtime.asleep} focus={inFocus(runtime.settings, Date.now())}
          disabled={writeBlocked || awaitingLiveState} onOpen={openAttention} backlog={runtime.backlog} quieted={runtime.quieted}
          onFinishFollowUp={(item) => { if (item.followUp) finishFollowUp(item.followUp) }} onSeen={(item) => { if (item.resurfaced) seeResurfaced(item.resurfaced) }}
          status={lastDelegationKind === 'signal' || lastDelegationKind === 'confirm' ? delegationControls : undefined} onDismiss={dismissSignal}
          onAnswer={(item, text) => { void quickAnswer(item, text) }} onApprove={(item) => { void quickApprove(item) }} onConfirm={quickConfirm} />
        {#if lastQuick === 'answer' && (pendingHuman || humanBusy || humanError || humanReceipt)}
          <div role="status" class="panel-description">
            {humanBusy ? '正在提交回答…' : humanError ? `回答未确认：${humanError}` : humanReceipt ? (humanReceipt.proposal ? `已保存为待批准提案 ${humanReceipt.proposal}，尚未生效。` : '回答已记录，等待受托者读取。') : ''}
            {#if pendingHuman && !humanBusy}<button onclick={() => { void sendHuman() }}>重试原回答</button>{/if}
          </div>
        {:else if lastQuick === 'approve' && (pendingApproval || approvalBusy || approvalError || approvalResult)}
          <div role="status" class="panel-description">
            {approvalBusy ? '正在核对并出版提案…' : approvalError ? `批准未确认：${approvalError}` : approvalResult}
            {#if pendingApproval && !approvalBusy}<button onclick={() => { void sendApproval() }}>重试原批准</button>{/if}
          </div>
        {/if}
        {#if lastRhythm && lastRhythm !== 'add' && (pendingRhythm || rhythmBusy || rhythmError || rhythmReceipt)}
          <div role="status" class="panel-description">
            {rhythmBusy ? '正在提交…' : rhythmError ? `未确认：${rhythmError}` : rhythmReceipt}
            {#if pendingRhythm && !rhythmBusy}<button onclick={() => { void sendRhythm() }}>重试原请求</button>{/if}
          </div>
        {/if}
        {#if runtime.rhythm}
          <FollowUps followUps={runtime.followUps} targets={followUpTargets} disabled={writeBlocked || awaitingLiveState}
            busy={rhythmBusy} frozen={pendingRhythm?.kind === 'add'} onAdd={addFollowUp} onFinish={(entry: FollowUpEntry) => finishFollowUp(entry.id)}
            error={lastRhythm === 'add' ? rhythmError : null} receipt={lastRhythm === 'add' ? rhythmReceipt : null}
            onRetry={() => { void sendRhythm() }} />
        {/if}
        {#if runtime.questions.length}
          <details><summary>问答记录（{runtime.questions.length}）</summary>
            {#each runtime.questions as question}
              <h3>{question.draft.text}</h3>
              {#if runtime.answers[question.draft.id]}
                {@const answer = runtime.answers[question.draft.id]}
                <p>{answer.input.text}</p>
                <p class="panel-description">{answer.publication ? `已随 ${answer.publication.baseline} 出版 · ${answer.publication.proposal}` : answer.proposal ? `已转为待批准提案 ${answer.proposal}` : answer.input.remember_as.kind === 'module_decision' ? `已记录模块决策 ${answer.input.remember_as.id ?? ''}` : answer.actor === 'process:towermd-cli' ? '会话转述（仅本次）' : '已记录本次回答'} · {answer.actor} · {answer.answered_at}</p>
              {:else}<button onclick={() => openHuman({ kind: 'answer', question })} disabled={writeBlocked}>回答 {question.draft.id}</button>{/if}
            {/each}
          </details>
        {/if}
        {#if runtime.prompts.length}
          <details><summary>插话记录（{runtime.prompts.length}）</summary>
            {#each runtime.prompts as prompt}
              <p>{prompt.text}</p>
              <p class="panel-description">{prompt.scope.kind === 'global' ? '全局' : prompt.scope.kind === 'module' ? `模块 ${prompt.scope.module}` : `任务 ${prompt.scope.task}`} · {promptSummary(prompt)}</p>
              <ul class="diagnostics">{#each prompt.recipients as recipient}<li>{recipient.run} · {promptState(prompt, recipient.run)}{prompt.handled[recipient.run] ? `：${prompt.handled[recipient.run]}` : ''}</li>{/each}</ul>
            {/each}
          </details>
        {/if}
        <TaskDetails tasks={runtime.tasks} runs={runtime.runs} positions={taskPositions} scope="all" delegation={delegationControls} focus={taskFocus} rhythm={rhythmMarks(runtime.rhythm)} {runDetails} {detailsLoading} {detailsError} onRunDetails={(id) => { void loadRunDetails(id) }} onPrompt={(task, initialText) => openHuman({ kind: 'prompt', scope: { kind: 'task', task }, initialText })} />
        <p class="source-footnote">来源 · Hub 已接受事件 {projectState.seq ?? ''}</p>
      {:else if stateLoading}<p role="status">正在读取任务…</p>
      {:else}<p class="panel-description">{projectState?.status === 'needs_recovery' ? '项目状态需要恢复，请在项目态势中处理。' : '当前项目尚未启用实时任务。'}</p>{/if}
    </aside>
  {/if}

  {#if !sample && showLedger && view}
    <aside class="canvas-panel" aria-label="信任账本">
      <div class="panel-heading"><h2>信任账本</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if stateError || connectionError}<p role="alert">状态暂未更新：{stateError || connectionError}</p>{/if}
      {#if delegationControls}<TrustLedger controls={delegationControls} />
      {:else if stateLoading}<p role="status">正在读取信任账本…</p>
      {:else}<p class="panel-description">{projectState?.status === 'ready' ? '守护进程未提供委托读模型，请更新。' : '当前项目尚未启用实时任务。'}</p>{/if}
    </aside>
  {/if}

  {#if !sample && mainResolution && mainResolution.token === project?.token}
    {#key `${mainResolution.token}:${mainResolution.commit}`}
      <MainResolution {client} token={mainResolution.token} commit={mainResolution.commit}
        visible={showMainResolution} onBlocked={(value) => { mainResolutionBlocked = value }}
        onClose={closeOverlays} onComplete={(seq) => { void mainResolutionCompleted(seq) }} />
    {/key}
  {/if}

  {#if !sample && proposalsOpened && project}
    {#key project.token}
      <Proposals {client} token={project.token} visible={showProposals}
        onBlocked={(value) => { proposalBlocked = value }} onClose={closeOverlays}
        onComplete={() => { void refreshAuthority() }} onViewDraft={(id) => { closeOverlays(); void loadDraft(id) }} />
    {/key}
  {/if}

  {#if !sample && showHuman && humanContext}
    <aside class="canvas-panel" aria-label={humanContext.kind === 'answer' ? '人工回答' : '人工插话'}>
      <div class="panel-heading"><h2>{humanContext.kind === 'answer' ? '回答问题' : '插话'}</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      {#if humanReceipt}
        <p class="panel-description">{humanReceipt.proposal ? `已保存为待批准提案 ${humanReceipt.proposal}，尚未生效。` : humanRule ? `临时规则已生效，${humanRule.until} 前每次调用 towermd 都会送达。` : humanContext.kind === 'answer' ? '回答已记录，等待受托者读取。' : '插话已记录，等待目标运行读取。'}</p>
        <dl class="source-list"><dt>回执</dt><dd>{humanReceipt.event}</dd><dt>序号</dt><dd>{humanReceipt.seq}</dd></dl>
      {:else}
        {#if awaitingLiveState}<p role="status">操作已记录，正在同步最新状态…</p>{/if}
        {#key humanContext.kind === 'answer' ? humanContext.question.draft.id : JSON.stringify(humanContext.scope)}
          <HumanMessage question={humanQuestion} scopeLabel={humanScope} initialText={humanContext.kind === 'prompt' ? humanContext.initialText : undefined}
            destinations={[{ value: 'temporary', label: '仅本次回答' }, { value: 'module_decision', label: '记为模块决策' }]}
            frozen={pendingHuman ? frozenHuman(pendingHuman) : null} commands={QUICK_COMMANDS.filter((command) => command.where === humanContext?.kind)}
            rules={humanContext.kind === 'prompt' ? humanRules : []}
            busy={humanBusy || awaitingLiveState} error={humanError} onSubmit={submitHuman} onRetry={() => { void sendHuman() }} />
        {/key}
      {/if}
    </aside>
  {/if}

  {#if !sample && (selected || showProject) && view}
    <aside class="canvas-panel" aria-label={selected ? '节点详情' : '项目态势'}>
      <div class="panel-heading">
        <h2>{selected?.title || view.title}</h2>
        <button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button>
      </div>
      {#if selected}
        <dl class="source-list">
          <dt>类型</dt><dd>{selected.object_type || selected.kind}</dd>
          {#if selected.object_id}<dt>对象</dt><dd>{selected.object_id}</dd>{/if}
          {#if selected.file}<dt>文件</dt><dd>{selected.file}</dd>{/if}
          {#if selected.url}<dt>链接</dt><dd>{selected.url}</dd>{/if}
        </dl>
        {#if !draft && !candidate && (selected.object_type === 'Module' || moduleTasks.length > 0)}
          <AttentionList items={moduleAttention} disabled={writeBlocked} onOpen={openAttention} />
          <button onclick={() => { void openTaskCreate(selected!.object_id || selected!.id) }} disabled={writeBlocked || projectState?.status !== 'ready'}>在模块新建委托</button>
          <button onclick={() => { showModuleTasks = !showModuleTasks }}>{showModuleTasks ? '收起模块任务' : `查看模块任务（${moduleTasks.length}）`}</button>
          <button onclick={() => openHuman({ kind: 'prompt', scope: { kind: 'module', module: selected!.object_id || selected!.id } })} disabled={writeBlocked || projectState?.status !== 'ready'}>向模块插话</button>
          {#if showModuleTasks}
            {#if stateError || connectionError}<p role="alert">状态暂未更新：{stateError || connectionError}</p>{/if}
            {#if projectState?.status === 'ready'}
              <TaskDetails tasks={moduleTasks} runs={runtime.runs} positions={taskPositions} scope={selected.object_id || selected.id} delegation={delegationControls} {runDetails} {detailsLoading} {detailsError} onRunDetails={(id) => { void loadRunDetails(id) }} onPrompt={(task, initialText) => openHuman({ kind: 'prompt', scope: { kind: 'task', task }, initialText })} />
            {:else}<p class="panel-description">当前实时任务状态不可用。</p>{/if}
          {/if}
        {/if}
        {#if !draft && !candidate && delegationControls && projectState?.status === 'ready'}
          <AskPanel target={selected.object_id || selected.id} controls={delegationControls} showStatus={lastDelegationKind === 'ask' || lastDelegationKind === 'withdraw'} />
        {/if}
        {#if draft}
          <button onclick={() => { editing = !editing }} disabled={draft.stale || writeBlocked}>{editing ? '查看原文' : '编辑草稿'}</button>
          {#if editing}{#key `${draft.id}:${selected.id}`}<DraftEditor {draft} node={selected} disabled={writeBlocked} onSubmit={(action) => { void saveEdit(action) }} />{/key}{/if}
          {#if draft.review?.pending_modules.includes(selected.object_id || selected.id)}
            <p class="verification-status verification-pending">◷ 待重新验证：旧结果不证明当前模块。</p>
          {/if}
        {/if}
        {#if selectedModule}
          {#if !draft}<dl class="source-list"><dt>置信度</dt><dd>{Math.round(selectedModule.confidence * 100)}%</dd><dt>依据</dt><dd>{selectedModule.rationale}</dd></dl>{/if}
          <p class="panel-description">{draft?.review ? '综合草稿 · 已含人工修订 · 未批准' : draft ? '综合草稿 · 结构与解释由提交者自报 · 未批准' : '候选模块 · 未批准'}</p>
          {#if detailReport}<VerificationDetails historical={!!draft?.review?.pending_modules.includes(selectedModule.id)} report={detailReport} moduleId={selectedModule.id} evidenceError={draft ? baselineEvidenceError(draft) : null} running={!draft && jobActive && activeJob?.id === detailReport.id} />{/if}
        {/if}
        {#if draft}
          {#if draft.stale}<p class="verification-status verification-unverifiable">! 来源提交已过期，此草稿只供查看。</p>{/if}
          {#if draft.review}
            <p class="panel-description">综合来源（自报）：{draft.producer}</p>
            <p class="panel-description">{draft.review.verification ? '修订保存' : '本次审阅'}：{draft.review.verification ? 'process:towermd-verifier' : draft.review.actor} · {draft.created_at} · 未批准</p>
          {:else}
            <p class="panel-description">提交者（自报）：{draft.producer} · {draft.created_at} · 未批准</p>
          {/if}
        {/if}
        <pre class="document-content">{selected.content || '此节点没有正文。'}</pre>
      {:else}
        <p class="panel-description">当前显示仓库提交中的画布。可平移、缩放和查看原文。</p>
        <p class="panel-description">当前版本只读。修改进入草稿，人工批准后才会出版。</p>
        {#if stateLoading}<p role="status">正在读取项目态势…</p>{/if}
        {#if stateError || connectionError}<p role="alert">{stateError || connectionError}</p><button onclick={openProjectState}>重试读取态势</button>{/if}
        {#if projectState}
          <p class="panel-description">{projectState.status === 'ready' ? (connectionError ? '显示上次状态' : '实时状态可用') : projectState.status === 'needs_recovery' ? '需要恢复项目状态' : projectState.status === 'hub_offline' ? 'hub 离线，暂时无法读取' : '尚未启用实时状态'}{projectState.seq !== undefined ? ` · 事件 ${projectState.seq}` : ''}</p>
          {#if projectState.error}<p role="alert">{projectState.error}</p>{/if}
          {#each projectState.warnings ?? [] as warning}<p role="alert">{warning}</p>{/each}
          {#if projectState.status === 'needs_recovery' && !recovery}
            <button onclick={requestRecovery} disabled={writeBlocked || busy}>准备恢复</button>
          {/if}
        {/if}
        {#if recovery}
          <p class="panel-description">恢复将从仓库快照和已确认事件重建状态，并使旧的模块占用失效。受托者需要重新领取任务。</p>
          <button class="primary-button" onclick={confirmRecovery} disabled={interactionWriteBlocked || recoveryBusy || publicationBusy || publicationAttempted || editBusy || !!pendingEdit || verificationBusy || !!pendingVerification}>{recoveryBusy ? '正在确认恢复…' : recoveryAttempted ? '重试原恢复确认' : '确认恢复项目状态'}</button>
          {#if !recoveryAttempted}<button onclick={() => { recovery = null; recoveryError = null }} disabled={recoveryBusy}>取消恢复确认</button>{/if}
        {/if}
        {#if recoveryError}<p role="alert">{recoveryError}</p>{/if}
        {#if recoveryAttempted && recoveryError}<p class="panel-description">恢复结果尚未确认；重试将沿用原请求号。</p>{/if}
        {#if recoveryCommit}<p class="panel-description">恢复已完成。</p><code>{recoveryCommit}</code>{/if}
        {#if published}
          <h3>{published.target} 已出版</h3>
          <dl class="source-list"><dt>标签</dt><dd>{published.tag}</dd><dt>提交</dt><dd><code>{published.commit}</code></dd><dt>快照</dt><dd><code>{published.snapshot_commit}</code></dd></dl>
        {/if}
        <dl class="source-list">
          <dt>项目</dt><dd>{project?.path}</dd>
          <dt>文档目录</dt><dd>{view.docs_dir || '尚未初始化'}</dd>
          <dt>画布</dt><dd>{view.nodes.length} 个节点 · {view.edges.length} 条关系</dd>
        </dl>
        {#if view.unplaced.length}
          <h3>未摆放模块</h3>
          <p class="panel-description">这些模块还没有画布节点。</p>
          <div class="unplaced-list">
            {#each view.unplaced as node}
              <button onclick={() => openNode(node)}>{node.title}</button>
            {/each}
          </div>
        {/if}
        {#if view.diagnostics.length}
          <h3>读取说明</h3>
          <ul class="diagnostics">{#each view.diagnostics as diagnostic}<li>{diagnostic}</li>{/each}</ul>
        {/if}
        <button onclick={chooseProject} disabled={writeBlocked}>切换项目</button>
      {/if}
      <div class="source-footnote">
        <span>来源 · {selected && (candidate || draft) ? displayed?.reference : view.reference}</span>
        <code>{selected && (candidate || draft) ? displayed?.revision : view.revision}</code>
      </div>
    </aside>
  {/if}
</main>
