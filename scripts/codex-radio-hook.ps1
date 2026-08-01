param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$RadioPath,

    [Parameter(Position = 1)]
    [string]$ThreadId = $env:CODEX_THREAD_ID
)

$ErrorActionPreference = 'Stop'
$utf8NoBom = [Text.UTF8Encoding]::new($false)

if (-not (Test-Path -LiteralPath $RadioPath)) {
    exit 0
}

$resolvedRadioPath = (Resolve-Path -LiteralPath $RadioPath).Path
$sessionDir = Split-Path -Parent $resolvedRadioPath
$statePath = Join-Path $sessionDir 'chat-radio-state.json'
$hookLogPath = Join-Path $sessionDir 'codex-radio-hook.log'

function Write-HookLog {
    param([string]$Message)

    $line = "[$([DateTimeOffset]::Now.ToString('o'))] $Message$([Environment]::NewLine)"
    [IO.File]::AppendAllText($hookLogPath, $line, $utf8NoBom)
}

function Save-ProcessedState {
    param(
        [Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][string]$Result
    )

    $nextState = [ordered]@{
        last_spoken_at_unix_ms = [int64]$Record.spoken_at_unix_ms
        last_message = [string]$Record.message
        result = $Result
        updated_at_iso = [DateTimeOffset]::Now.ToString('o')
    }
    $json = $nextState | ConvertTo-Json
    $temporaryPath = "$statePath.tmp"
    [IO.File]::WriteAllText($temporaryPath, $json, $utf8NoBom)
    Move-Item -LiteralPath $temporaryPath -Destination $statePath -Force
}

if ([string]::IsNullOrWhiteSpace($ThreadId)) {
    Write-HookLog 'result=missing_thread_id'
    throw 'CODEX_THREAD_ID is not set. Start the bridge from the Codex task terminal or pass this task id as the second argument.'
}

$stateExists = Test-Path -LiteralPath $statePath
$lastSpokenAt = [int64]0
if ($stateExists) {
    try {
        $savedState = Get-Content -Raw -Encoding utf8 -LiteralPath $statePath | ConvertFrom-Json
        if ($null -ne $savedState.last_spoken_at_unix_ms) {
            $lastSpokenAt = [int64]$savedState.last_spoken_at_unix_ms
        }
    } catch {
        Write-HookLog "state_reset reason=invalid_state error=$($_.Exception.Message)"
        $stateExists = $false
    }
}

$allRecords = @(
    Get-Content -Encoding utf8 -LiteralPath $resolvedRadioPath |
        Where-Object { $_.Trim().Length -gt 0 } |
        ForEach-Object { $_ | ConvertFrom-Json } |
        Sort-Object { [int64]$_.spoken_at_unix_ms }
)

$newRecords = if (-not $stateExists -and $allRecords.Count -gt 0) {
    @($allRecords[-1])
} else {
    @($allRecords | Where-Object { [int64]$_.spoken_at_unix_ms -gt $lastSpokenAt })
}

if ($newRecords.Count -eq 0) {
    exit 0
}

$latestSeen = $newRecords[-1]
$actionableRecords = @(
    $newRecords | Where-Object { $_.kind -notin @('front_gap', 'behind_gap') }
)
if ($actionableRecords.Count -eq 0) {
    Save-ProcessedState -Record $latestSeen -Result 'ignored_gap'
    Write-HookLog "result=ignored_gap kind=$($latestSeen.kind)"
    exit 0
}
$latest = $actionableRecords[-1]

$engineerStatePath = Join-Path $sessionDir 'state.json'
if (-not (Test-Path -LiteralPath $engineerStatePath)) {
    Save-ProcessedState -Record $latestSeen -Result 'missing_state'
    Write-HookLog 'result=missing_state'
    exit 0
}

$engineerState = Get-Content -Raw -Encoding utf8 -LiteralPath $engineerStatePath | ConvertFrom-Json
$sameSession = [string]$engineerState.session_uid -eq [string]$latest.session_uid
$sameTimeline = [uint64]$engineerState.timeline_revision -eq [uint64]$latest.timeline_revision
$stateAgeMs = [Math]::Abs([double]$engineerState.updated_at_unix_ms - [double]$latest.spoken_at_unix_ms)
if ($engineerState.lifecycle.status -ne 'active' -or -not $sameSession -or -not $sameTimeline -or $stateAgeMs -gt 15000) {
    Save-ProcessedState -Record $latestSeen -Result 'stale_or_inactive'
    Write-HookLog "result=stale_or_inactive lifecycle=$($engineerState.lifecycle.status) same_session=$sameSession same_timeline=$sameTimeline state_age_ms=$([int]$stateAgeMs)"
    exit 0
}

$wear = $engineerState.damage.tyre_wear
$nearForecast = @(
    $engineerState.session.weather_forecast_samples |
        Where-Object { [int]$_.time_offset_min -le 10 } |
        Select-Object -First 3
)
$recentLaps = @($engineerState.race_strategy.recent_laps | Select-Object -First 3)
$eventContext = [ordered]@{
    priority = $latest.priority
    kind = $latest.kind
    message = $latest.message
    spoken_at_unix_ms = $latest.spoken_at_unix_ms
}
$stateContext = [ordered]@{
    updated_at_unix_ms = $engineerState.updated_at_unix_ms
    lap = $engineerState.lap.current_lap_num
    total_laps = $engineerState.session.total_laps
    position = $engineerState.lap.car_position
    pit_status = $engineerState.lap.pit_status
    safety_car_status = $engineerState.session.safety_car_status
    weather = $engineerState.session.weather
    weather_forecast_next_10_min = $nearForecast
    pit_window = [ordered]@{
        ideal_lap = $engineerState.session.pit_stop_window_ideal_lap
        latest_lap = $engineerState.session.pit_stop_window_latest_lap
        predicted_rejoin_position = $engineerState.session.pit_stop_rejoin_position
    }
    fuel_delta_laps = $engineerState.status.fuel_delta_laps
    ers_percent = $engineerState.ers.store_percent
    tyre_compound = $engineerState.status.visual_tyre_compound
    tyre_age_laps = $engineerState.status.tyres_age_laps
    tyre_wear_percent = [ordered]@{
        fl = $wear.fl
        fr = $wear.fr
        rl = $wear.rl
        rr = $wear.rr
    }
    damage = [ordered]@{
        front_left_wing = $engineerState.damage.front_left_wing_damage
        front_right_wing = $engineerState.damage.front_right_wing_damage
        rear_wing = $engineerState.damage.rear_wing_damage
        gearbox = $engineerState.damage.gearbox_damage
        engine = $engineerState.damage.engine_damage
    }
    timing_gap_audit_only = [ordered]@{
        front_gap_ms = $engineerState.lap.delta_to_car_in_front_ms
        front_car_index = $engineerState.lap.car_in_front_index
        behind_gap_ms = $engineerState.lap.delta_to_car_behind_ms
        behind_car_index = $engineerState.lap.car_behind_index
    }
    strategy = [ordered]@{
        representative_stint_laps = $engineerState.race_strategy.representative_stint_laps
        pace_trend_s_per_lap = $engineerState.race_strategy.pace_trend_s_per_lap
        limiting_tyre = $engineerState.race_strategy.limiting_tyre
        projected_finish_wear_percent = $engineerState.race_strategy.projected_finish_wear_percent
        recent_laps = $recentLaps
    }
}
$eventJson = $eventContext | ConvertTo-Json -Compress -Depth 5
$stateJson = $stateContext | ConvertTo-Json -Compress -Depth 8
$prompt = @"
<spoken_event>
$eventJson
</spoken_event>
<current_race_state>
$stateJson
</current_race_state>
This existing Codex task is the user's live AI race-engineer session. Treat the current race state above as authoritative and answer directly in this task. Do not use tools, inspect files, or use outside facts. Prioritize race control, damage, pit timing, fuel, tyre condition, and ERS in that order. Never infer opponent tyres or pit-loss details that are absent. The timing gap fields are audit-only and unreliable around passes: never issue or correct an attack, defend, front-gap, or behind-gap call from them. Do not repeat the spoken event. Give only a correction or the next action that matters within the next one to three laps; if no correction is needed, confirm the current plan briefly. Reply in one or two short Korean sentences prefixed exactly with `AI engineer:`.
"@

$codexBinRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\bin'
$codexCli = Get-ChildItem -LiteralPath $codexBinRoot -Recurse -Filter codex.exe -ErrorAction Stop |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1 -ExpandProperty FullName
if ([string]::IsNullOrWhiteSpace($codexCli)) {
    Save-ProcessedState -Record $latestSeen -Result 'codex_missing'
    throw "Codex CLI was not found below $codexBinRoot"
}

$responsePath = Join-Path $sessionDir ('.codex-ai-response-' + [Guid]::NewGuid().ToString('N') + '.txt')
$codexArgs = @(
    'exec',
    'resume',
    '--ignore-user-config',
    '--ignore-rules',
    '--skip-git-repo-check',
    '-c',
    'sandbox_mode="read-only"',
    '-c',
    'approval_policy="never"',
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

$response = ''
if (Test-Path -LiteralPath $responsePath) {
    try {
        $response = Get-Content -Raw -Encoding utf8 -LiteralPath $responsePath
    } finally {
        [IO.File]::Delete($responsePath)
    }
}
$response = (($response -replace '\s+', ' ').Trim())

if ([string]::IsNullOrWhiteSpace($response)) {
    $diagnostic = $commandOutput.Trim()
    if ($diagnostic.Length -gt 2000) {
        $diagnostic = $diagnostic.Substring($diagnostic.Length - 2000)
    }
    Save-ProcessedState -Record $latestSeen -Result 'codex_failed'
    Write-HookLog "result=codex_failed exit=$codexExitCode diagnostic=$diagnostic"
    throw "Codex AI radio exited with status $codexExitCode"
}

Save-ProcessedState -Record $latestSeen -Result 'delivered_to_task'
Write-HookLog "result=delivered_to_task kind=$($latest.kind) exit=$codexExitCode response_length=$($response.Length)"
