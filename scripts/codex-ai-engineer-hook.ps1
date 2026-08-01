param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$TriggerPath,

    [Parameter(Position = 1)]
    [string]$ThreadId
)

$ErrorActionPreference = 'Stop'
$utf8NoBom = [Text.UTF8Encoding]::new($false)

if (-not (Test-Path -LiteralPath $TriggerPath)) {
    exit 0
}

$resolvedTriggerPath = (Resolve-Path -LiteralPath $TriggerPath).Path
$sessionDir = Split-Path -Parent $resolvedTriggerPath
$hookLogPath = Join-Path $sessionDir 'codex-ai-engineer-hook.log'
$workerStatePath = Join-Path $sessionDir 'ai-engineer-state.json'
$radioLogPath = Join-Path $sessionDir 'ai-engineer-radio.jsonl'
$schemaPath = Join-Path $PSScriptRoot 'ai-engineer-decision.schema.json'

function Write-HookLog {
    param([string]$Message)

    $line = "[$([DateTimeOffset]::Now.ToString('o'))] $Message$([Environment]::NewLine)"
    [IO.File]::AppendAllText($hookLogPath, $line, $utf8NoBom)
}

function Save-WorkerState {
    param(
        [Parameter(Mandatory = $true)]$Trigger,
        [Parameter(Mandatory = $true)][string]$Result
    )

    $state = [ordered]@{
        session_uid = [string]$Trigger.state.session_uid
        timeline_revision = [uint64]$Trigger.timeline_revision
        sequence = [uint64]$Trigger.sequence
        result = $Result
        updated_at_iso = [DateTimeOffset]::Now.ToString('o')
    }
    $temporaryPath = "$workerStatePath.tmp"
    [IO.File]::WriteAllText($temporaryPath, ($state | ConvertTo-Json), $utf8NoBom)
    Move-Item -LiteralPath $temporaryPath -Destination $workerStatePath -Force
}

if ([string]::IsNullOrWhiteSpace($ThreadId) -and
    -not [string]::IsNullOrWhiteSpace($env:SIM_MOZA_ENGINEER_TASK_ID)) {
    $ThreadId = $env:SIM_MOZA_ENGINEER_TASK_ID
}
if ([string]::IsNullOrWhiteSpace($ThreadId) -and
    -not [string]::IsNullOrWhiteSpace($env:CODEX_THREAD_ID)) {
    $ThreadId = $env:CODEX_THREAD_ID
}
if ([string]::IsNullOrWhiteSpace($ThreadId)) {
    Write-HookLog 'result=missing_thread_id'
    throw 'No AI engineer task id is configured. Pass it as the second argument, set SIM_MOZA_ENGINEER_TASK_ID, or start from the target Codex task.'
}
if (-not (Test-Path -LiteralPath $schemaPath)) {
    throw "AI decision schema was not found: $schemaPath"
}

$trigger = Get-Content -Raw -Encoding utf8 -LiteralPath $resolvedTriggerPath | ConvertFrom-Json
if ([int]$trigger.schema_version -lt 4 -or
    [int]$trigger.state.schema_version -lt 8 -or
    [string]$trigger.decision_mode -ne 'ai' -or
    $null -eq $trigger.state.radio_revisions) {
    Write-HookLog "result=ignored_contract trigger_schema=$($trigger.schema_version) state_schema=$($trigger.state.schema_version) mode=$($trigger.decision_mode)"
    exit 0
}

if (Test-Path -LiteralPath $workerStatePath) {
    try {
        $previous = Get-Content -Raw -Encoding utf8 -LiteralPath $workerStatePath | ConvertFrom-Json
        $sameSession = [string]$previous.session_uid -eq [string]$trigger.state.session_uid
        $sameTimeline = [uint64]$previous.timeline_revision -eq [uint64]$trigger.timeline_revision
        if ($sameSession -and $sameTimeline -and [uint64]$previous.sequence -ge [uint64]$trigger.sequence) {
            exit 0
        }
    } catch {
        Write-HookLog "state_reset reason=invalid_state error=$($_.Exception.Message)"
    }
}

$passiveReasons = @(
    'engineer_online',
    'timeline_reset',
    'position',
    'front_gap',
    'behind_gap',
    'race_strategy_snapshot'
)
$reasons = @($trigger.reasons | ForEach-Object { [string]$_ })
$decisionReasons = @($reasons | Where-Object { $_ -notin $passiveReasons })
if ($decisionReasons.Count -eq 0) {
    Save-WorkerState -Trigger $trigger -Result 'ignored_observation'
    Write-HookLog "result=ignored_observation sequence=$($trigger.sequence)"
    exit 0
}

if ([string]$trigger.state.lifecycle.status -ne 'active' -or -not [bool]$trigger.state.driving) {
    Save-WorkerState -Trigger $trigger -Result 'inactive'
    Write-HookLog "result=inactive sequence=$($trigger.sequence) lifecycle=$($trigger.state.lifecycle.status)"
    exit 0
}

$availableTyres = @(
    $trigger.state.tyre_sets.sets |
        Where-Object { [bool]$_.available } |
        Select-Object index, visual_tyre_compound, wear_percent, life_span_laps, usable_life_laps, lap_delta_ms, fitted
)
$inputSummary = [ordered]@{
    speed_kmh = $trigger.state.input.speed_kmh
    throttle = $trigger.state.input.throttle
    brake = $trigger.state.input.brake
    steer = $trigger.state.input.steer
    gear = $trigger.state.input.gear
}
$lapSummary = [ordered]@{
    current_lap_num = $trigger.state.lap.current_lap_num
    total_laps = $trigger.state.session.total_laps
    car_position = $trigger.state.lap.car_position
    pit_status = $trigger.state.lap.pit_status
    num_pit_stops = $trigger.state.lap.num_pit_stops
    current_lap_invalid = $trigger.state.lap.current_lap_invalid
    sector = $trigger.state.lap.sector
}
$stateForAi = [ordered]@{
    source = $trigger.state.source
    session_uid = [string]$trigger.state.session_uid
    timeline_revision = [uint64]$trigger.timeline_revision
    lifecycle = $trigger.state.lifecycle.status
    session_type = $trigger.state.session_type_name
    observations = $decisionReasons
    completed_lap = $trigger.completed_lap
    input = $inputSummary
    lap = $lapSummary
    session = $trigger.state.session
    status = $trigger.state.status
    ers = $trigger.state.ers
    damage = $trigger.state.damage
    race_strategy = $trigger.state.race_strategy
    practice_program = $trigger.state.practice_program
    available_tyre_sets = $availableTyres
}
$stateJson = $stateForAi | ConvertTo-Json -Compress -Depth 10
$prompt = @"
<ai_race_engineer_event>
$stateJson
</ai_race_engineer_event>
This Codex task is the user's designated live AI race-engineer session. Make the radio decision yourself from the telemetry metrics and recent-lap evidence; the observation names only explain why you were awakened and are not recommendations. Do not use tools, inspect files, or use outside facts. Return only JSON matching the supplied schema.

Decision policy:
- Be silent unless the driver needs a materially new action, correction, warning, or strategy update now or within three laps.
- Prioritize race control and immediate safety, then new damage, pit timing and traffic, tyre state, fuel, and ERS.
- Do not repeat a prior call or narrate raw metrics without an action.
- Never make an attack/defend call or quote a front/behind gap from timing deltas. Those values are deliberately omitted because they were unreliable around passes.
- Do not infer opponent tyres, pit loss, grip, weather, or strategy when the supplied state does not support it.
- Use concise natural Korean for message. Set speak=false, kind=none, and message="" when no call is justified.
- Echo trigger_sequence=$($trigger.sequence), session_uid="$([string]$trigger.state.session_uid)", and timeline_revision=$($trigger.timeline_revision) exactly.
- List the exact telemetry field names used in basis. Model latency consumes part of the TTL: use 30000 for durable strategy, damage, fuel, or ERS calls and never set valid_for_ms below 20000.
"@

$codexBinRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\bin'
$codexCli = Get-ChildItem -LiteralPath $codexBinRoot -Recurse -Filter codex.exe -ErrorAction Stop |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1 -ExpandProperty FullName
if ([string]::IsNullOrWhiteSpace($codexCli)) {
    Save-WorkerState -Trigger $trigger -Result 'codex_missing'
    throw "Codex CLI was not found below $codexBinRoot"
}

$responsePath = Join-Path $sessionDir ('.ai-engineer-decision-' + [Guid]::NewGuid().ToString('N') + '.json')
$codexArgs = @(
    'exec',
    'resume',
    '--ephemeral',
    '--ignore-user-config',
    '--ignore-rules',
    '--skip-git-repo-check',
    '-c',
    'sandbox_mode="read-only"',
    '-c',
    'approval_policy="never"',
    '--output-schema',
    $schemaPath,
    '-o',
    $responsePath,
    $ThreadId,
    '-'
)

$savedErrorActionPreference = $ErrorActionPreference
$savedOutputEncoding = $OutputEncoding
$savedConsoleOutputEncoding = [Console]::OutputEncoding
$ErrorActionPreference = 'Continue'
$OutputEncoding = $utf8NoBom
[Console]::OutputEncoding = $utf8NoBom
$commandOutput = ''
$codexExitCode = 1
try {
    $commandOutput = $prompt | & $codexCli @codexArgs 2>&1 | Out-String
    $codexExitCode = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $savedErrorActionPreference
    $OutputEncoding = $savedOutputEncoding
    [Console]::OutputEncoding = $savedConsoleOutputEncoding
}

$responseText = ''
if (Test-Path -LiteralPath $responsePath) {
    try {
        $responseText = Get-Content -Raw -Encoding utf8 -LiteralPath $responsePath
    } finally {
        [IO.File]::Delete($responsePath)
    }
}
if ([string]::IsNullOrWhiteSpace($responseText)) {
    $diagnostic = $commandOutput.Trim()
    if ($diagnostic.Length -gt 2000) {
        $diagnostic = $diagnostic.Substring($diagnostic.Length - 2000)
    }
    Save-WorkerState -Trigger $trigger -Result 'codex_failed'
    Write-HookLog "result=codex_failed sequence=$($trigger.sequence) exit=$codexExitCode diagnostic=$diagnostic"
    throw "Codex AI decision failed with status $codexExitCode"
}

$decision = $responseText | ConvertFrom-Json
$identityMatches = [uint64]$decision.trigger_sequence -eq [uint64]$trigger.sequence -and
    [string]$decision.session_uid -eq [string]$trigger.state.session_uid -and
    [uint64]$decision.timeline_revision -eq [uint64]$trigger.timeline_revision
if (-not $identityMatches) {
    Save-WorkerState -Trigger $trigger -Result 'identity_mismatch'
    Write-HookLog "result=identity_mismatch sequence=$($trigger.sequence)"
    exit 0
}

if (-not [bool]$decision.speak) {
    Save-WorkerState -Trigger $trigger -Result 'silent'
    Write-HookLog "result=silent sequence=$($trigger.sequence) exit=$codexExitCode reason=$($decision.reason)"
    exit 0
}

$message = (($decision.message -replace '\s+', ' ').Trim())
if ([string]::IsNullOrWhiteSpace($message) -or $message.Length -gt 180 -or [string]$decision.kind -in @('front_gap', 'behind_gap', 'position')) {
    Save-WorkerState -Trigger $trigger -Result 'rejected_decision'
    Write-HookLog "result=rejected_decision sequence=$($trigger.sequence) kind=$($decision.kind) length=$($message.Length)"
    exit 0
}

$liveStatePath = [string]$trigger.state_path
if ([string]::IsNullOrWhiteSpace($liveStatePath) -or -not (Test-Path -LiteralPath $liveStatePath)) {
    Save-WorkerState -Trigger $trigger -Result 'missing_live_state'
    Write-HookLog "result=missing_live_state sequence=$($trigger.sequence)"
    exit 0
}
$liveState = Get-Content -Raw -Encoding utf8 -LiteralPath $liveStatePath | ConvertFrom-Json
$sameSession = [string]$liveState.session_uid -eq [string]$trigger.state.session_uid
$sameTimeline = [uint64]$liveState.timeline_revision -eq [uint64]$trigger.timeline_revision
$elapsedMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [int64]$trigger.timestamp_unix_ms
$stillActive = [string]$liveState.lifecycle.status -eq 'active' -and [bool]$liveState.driving
$revisionGroups = [ordered]@{
    track_flag = @('yellow_flag', 'red_flag', 'green_flag')
    race_control = @('safety_car', 'virtual_safety_car', 'race_restart')
    conditions = @('rain_started', 'track_drying', 'weather_forecast')
    pit = @('pit_limiter')
    damage = @('tyre_wear', 'tyre_damage', 'front_wing_damage', 'rear_wing_damage', 'engine_damage', 'gearbox_damage')
    rival = @('rival_pit_front', 'rival_pit_behind', 'rival_pit_safety_car')
    strategy = @('lap_complete', 'lap_invalid', 'practice_program', 'strategy_stay_out', 'strategy_reassess', 'pit_window_open', 'pit_window_latest', 'fuel_target', 'ers_target', 'tyre_degradation')
}
$revisionsCurrent = $true
$staleRevisionGroup = ''
foreach ($entry in $revisionGroups.GetEnumerator()) {
    $usesGroup = @($decisionReasons | Where-Object { $_ -in $entry.Value }).Count -gt 0
    if (-not $usesGroup) {
        continue
    }
    $triggerProperty = $trigger.state.radio_revisions.PSObject.Properties[$entry.Key]
    $liveProperty = $liveState.radio_revisions.PSObject.Properties[$entry.Key]
    if ($null -eq $triggerProperty -or $null -eq $liveProperty -or
        [uint64]$triggerProperty.Value -ne [uint64]$liveProperty.Value) {
        $revisionsCurrent = $false
        $staleRevisionGroup = $entry.Key
        break
    }
}
$raceControlCurrent = $true
if ($decisionReasons -contains 'safety_car' -or $decisionReasons -contains 'virtual_safety_car') {
    $raceControlCurrent = [int]$liveState.session.safety_car_status -ne 0
} elseif ($decisionReasons -contains 'race_restart') {
    $raceControlCurrent = [int]$liveState.session.safety_car_status -eq 0
}
if (-not $sameSession -or -not $sameTimeline -or -not $stillActive -or -not $revisionsCurrent -or -not $raceControlCurrent -or $elapsedMs -gt [int64]$decision.valid_for_ms) {
    Save-WorkerState -Trigger $trigger -Result 'stale_decision'
    Write-HookLog "result=stale_decision sequence=$($trigger.sequence) elapsed_ms=$elapsedMs same_session=$sameSession same_timeline=$sameTimeline active=$stillActive revisions_current=$revisionsCurrent stale_revision_group=$staleRevisionGroup race_control_current=$raceControlCurrent"
    exit 0
}

$speaker = $null
try {
    Add-Type -AssemblyName System.Speech
    $speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $koreanVoice = $speaker.GetInstalledVoices() |
        Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -eq 'ko-KR' } |
        Select-Object -First 1
    if ($null -ne $koreanVoice) {
        $speaker.SelectVoice($koreanVoice.VoiceInfo.Name)
    }
    $speaker.Rate = 1
    $speaker.Volume = 100
    $speaker.Speak($message)
} finally {
    if ($null -ne $speaker) {
        $speaker.Dispose()
    }
}

$record = [ordered]@{
    schema_version = 1
    spoken_at_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    trigger_sequence = [uint64]$trigger.sequence
    source = $trigger.state.source
    session_uid = [string]$trigger.state.session_uid
    timeline_revision = [uint64]$trigger.timeline_revision
    lap = $liveState.lap.current_lap_num
    position = $liveState.lap.car_position
    priority = $decision.priority
    kind = $decision.kind
    message = $message
    basis = $decision.basis
    reason = $decision.reason
}
[IO.File]::AppendAllText($radioLogPath, (($record | ConvertTo-Json -Compress -Depth 5) + [Environment]::NewLine), $utf8NoBom)
Save-WorkerState -Trigger $trigger -Result 'spoken'
Write-HookLog "result=spoken sequence=$($trigger.sequence) kind=$($decision.kind) exit=$codexExitCode"
