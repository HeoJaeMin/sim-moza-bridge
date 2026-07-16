"use strict";

const LIVE_POLL_MS = 250;
const TRACE_POLL_MS = 1000;
const ANALYSIS_POLL_MS = 2000;
const SVG_NS = "http://www.w3.org/2000/svg";
const MAP_WIDTH = 1000;
const MAP_HEIGHT = 640;
const MAP_PADDING = 54;
const TRACK_MAX_GAP_M = 40;

const state = {
  live: null,
  liveTrace: null,
  savedLaps: [],
  selectedLapId: "live",
  displayedTrace: null,
  trackPathKey: "",
  trackProjection: null,
  contactsKey: "",
  lapOptionsKey: "",
  analysis: null,
  activeView: "live",
  controlBusy: false,
  shutdownRequested: false,
};

const elements = {
  connectionBadge: document.querySelector("#connection-badge"),
  connectionLabel: document.querySelector("#connection-label"),
  sourceLabel: document.querySelector("#source-label"),
  warningBanner: document.querySelector("#warning-banner"),
  warningText: document.querySelector("#warning-text"),
  liveTab: document.querySelector("#live-tab"),
  analysisTab: document.querySelector("#analysis-tab"),
  liveView: document.querySelector("#live-view"),
  analysisView: document.querySelector("#analysis-view"),
  pauseButton: document.querySelector("#pause-button"),
  resumeButton: document.querySelector("#resume-button"),
  shutdownButton: document.querySelector("#shutdown-button"),
  controlFeedback: document.querySelector("#control-feedback"),
  captureStrip: document.querySelector(".capture-strip"),
  captureState: document.querySelector("#capture-state"),
  captureRate: document.querySelector("#capture-rate"),
  captureAccepted: document.querySelector("#capture-accepted"),
  captureRejected: document.querySelector("#capture-rejected"),
  captureDuplicates: document.querySelector("#capture-duplicates"),
  captureStalled: document.querySelector("#capture-stalled"),
  captureInconsistent: document.querySelector("#capture-inconsistent"),
  captureInvalidSessions: document.querySelector("#capture-invalid-sessions"),
  captureTelemetryAccepted: document.querySelector("#capture-telemetry-accepted"),
  captureTelemetryRejected: document.querySelector("#capture-telemetry-rejected"),
  captureTelemetryDuplicates: document.querySelector("#capture-telemetry-duplicates"),
  captureTelemetryBackward: document.querySelector("#capture-telemetry-backward"),
  captureTelemetryDelayed: document.querySelector("#capture-telemetry-delayed"),
  captureTelemetrySudden: document.querySelector("#capture-telemetry-sudden"),
  captureAge: document.querySelector("#capture-age"),
  persistencePending: document.querySelector("#persistence-pending"),
  persistenceResult: document.querySelector("#persistence-result"),
  captureNote: document.querySelector("#capture-note"),
  sessionPhase: document.querySelector("#session-phase"),
  sessionTrack: document.querySelector("#session-track"),
  sessionType: document.querySelector("#session-type"),
  sessionRemaining: document.querySelector("#session-remaining"),
  sessionLap: document.querySelector("#session-lap"),
  sessionTemperature: document.querySelector("#session-temperature"),
  sessionCars: document.querySelector("#session-cars"),
  mapPointCount: document.querySelector("#map-point-count"),
  mapCarCount: document.querySelector("#map-car-count"),
  trackLayer: document.querySelector("#track-layer"),
  vehicleLayer: document.querySelector("#vehicle-layer"),
  trackEmpty: document.querySelector("#track-empty"),
  leaderboardBody: document.querySelector("#leaderboard-body"),
  leaderboardCount: document.querySelector("#leaderboard-count"),
  leaderboardEmpty: document.querySelector("#leaderboard-empty"),
  lapSelect: document.querySelector("#lap-select"),
  traceLap: document.querySelector("#trace-lap"),
  traceTime: document.querySelector("#trace-time"),
  traceDetail: document.querySelector("#trace-detail"),
  chart: document.querySelector("#telemetry-chart"),
  chartEmpty: document.querySelector("#chart-empty"),
  contactsList: document.querySelector("#contacts-list"),
  contactsCount: document.querySelector("#contacts-count"),
  contactsEmpty: document.querySelector("#contacts-empty"),
  analysisStatus: document.querySelector("#analysis-status"),
  analysisMessage: document.querySelector("#analysis-message"),
  analysisGenerated: document.querySelector("#analysis-generated"),
  analysisTrack: document.querySelector("#analysis-track"),
  analysisSession: document.querySelector("#analysis-session"),
  analysisClass: document.querySelector("#analysis-class"),
  cohortParticipants: document.querySelector("#cohort-participants"),
  cohortValidLaps: document.querySelector("#cohort-valid-laps"),
  cohortTopCount: document.querySelector("#cohort-top-count"),
  cohortTopMedian: document.querySelector("#cohort-top-median"),
  playerBenchmark: document.querySelector("#player-benchmark"),
  p1Benchmark: document.querySelector("#p1-benchmark"),
  fastestBenchmark: document.querySelector("#fastest-benchmark"),
  focusZones: document.querySelector("#focus-zones"),
  focusCount: document.querySelector("#focus-count"),
  focusEmpty: document.querySelector("#focus-empty"),
  segmentsBody: document.querySelector("#segments-body"),
  segmentsCount: document.querySelector("#segments-count"),
  segmentsEmpty: document.querySelector("#segments-empty"),
  exclusionsList: document.querySelector("#exclusions-list"),
  exclusionsEmpty: document.querySelector("#exclusions-empty"),
  limitationsList: document.querySelector("#limitations-list"),
  limitationsEmpty: document.querySelector("#limitations-empty"),
  lastUpdate: document.querySelector("#last-update"),
};

const chartContext = elements.chart.getContext("2d");

elements.lapSelect.addEventListener("change", () => {
  state.selectedLapId = elements.lapSelect.value;
  if (state.selectedLapId === "live") {
    state.displayedTrace = state.liveTrace;
    renderTrace(state.displayedTrace, true);
    return;
  }
  loadSavedLap(state.selectedLapId);
});

elements.liveTab.addEventListener("click", () => selectView("live"));
elements.analysisTab.addEventListener("click", () => selectView("analysis"));
elements.pauseButton.addEventListener("click", () => requestControl("pause"));
elements.resumeButton.addEventListener("click", () => requestControl("resume"));
elements.shutdownButton.addEventListener("click", () => {
  if (!window.confirm("수집을 종료하고 대기 중인 텔레메트리를 모두 저장할까요?")) return;
  requestControl("shutdown");
});

window.addEventListener("resize", () => renderTrace(state.displayedTrace, state.selectedLapId === "live"));

function pick(object, keys, fallback = undefined) {
  if (!object || typeof object !== "object") return fallback;
  for (const key of keys) {
    const value = object[key];
    if (value !== undefined && value !== null) return value;
  }
  return fallback;
}

function finiteNumber(value, fallback = null) {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function asArray(value) {
  return Array.isArray(value) ? value : [];
}

function setText(element, value) {
  const text = String(value);
  if (element.textContent !== text) element.textContent = text;
}

async function getJson(path) {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 4000);
  try {
    const response = await fetch(path, {
      cache: "no-store",
      headers: { Accept: "application/json" },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
    return await response.json();
  } finally {
    window.clearTimeout(timeout);
  }
}

async function postControl(action) {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 5000);
  try {
    const response = await fetch(`/api/control/${encodeURIComponent(action)}`, {
      method: "POST",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "X-LMU-Dashboard-Control": "1",
      },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
    const contentType = response.headers.get("content-type") || "";
    return contentType.includes("application/json") ? await response.json() : await response.text();
  } finally {
    window.clearTimeout(timeout);
  }
}

function selectView(view) {
  state.activeView = view === "analysis" ? "analysis" : "live";
  const analysisSelected = state.activeView === "analysis";
  elements.liveTab.classList.toggle("is-active", !analysisSelected);
  elements.analysisTab.classList.toggle("is-active", analysisSelected);
  elements.liveTab.setAttribute("aria-selected", String(!analysisSelected));
  elements.analysisTab.setAttribute("aria-selected", String(analysisSelected));
  elements.liveView.hidden = analysisSelected;
  elements.analysisView.hidden = !analysisSelected;
}

async function requestControl(action) {
  if (state.controlBusy || state.shutdownRequested) return;
  state.controlBusy = true;
  renderControlButtons(pick(state.live, ["capture"], {}));
  setControlFeedback(
    action === "pause" ? "수집을 일시정지하는 중입니다…" : action === "resume" ? "수집을 재개하는 중입니다…" : "저장 후 종료하는 중입니다…",
    false,
  );
  try {
    const result = await postControl(action);
    const serverMessage = typeof result === "object" && result ? pick(result, ["message"], "") : "";
    if (action === "shutdown") state.shutdownRequested = true;
    setControlFeedback(
      serverMessage ||
        (action === "pause"
          ? "수집을 일시정지했습니다."
          : action === "resume"
            ? "수집을 재개했습니다."
            : "대기 중인 데이터를 저장하고 대시보드를 종료합니다."),
      false,
    );
  } catch (error) {
    setControlFeedback(`제어 요청 실패: ${error instanceof Error ? error.message : String(error)}`, true);
  } finally {
    state.controlBusy = false;
    renderControlButtons(pick(state.live, ["capture"], {}));
  }
}

function setControlFeedback(message, error) {
  elements.controlFeedback.hidden = !message;
  elements.controlFeedback.classList.toggle("is-error", error);
  setText(elements.controlFeedback, message);
}

async function pollLive() {
  try {
    const live = await getJson("/api/live");
    state.live = live;
    renderLive(live);
  } catch (error) {
    renderConnection(false, "연결 오류", error instanceof Error ? error.message : String(error));
  } finally {
    window.setTimeout(pollLive, LIVE_POLL_MS);
  }
}

async function pollTraceAndLaps() {
  const [traceResult, lapsResult] = await Promise.allSettled([
    getJson("/api/trace"),
    getJson("/api/laps"),
  ]);

  if (traceResult.status === "fulfilled") {
    state.liveTrace = traceResult.value;
    if (state.selectedLapId === "live") {
      state.displayedTrace = state.liveTrace;
      renderTrace(state.displayedTrace, true);
    }
  }

  if (lapsResult.status === "fulfilled") {
    state.savedLaps = asArray(lapsResult.value);
    renderLapOptions();
  }

  window.setTimeout(pollTraceAndLaps, TRACE_POLL_MS);
}

async function pollAnalysis() {
  try {
    state.analysis = await getJson("/api/analysis");
    renderAnalysis(state.analysis);
  } catch (error) {
    if (!state.analysis) {
      renderAnalysis({
        status: "unavailable",
        message: `분석 데이터를 불러오지 못했습니다: ${error instanceof Error ? error.message : String(error)}`,
      });
    }
  } finally {
    window.setTimeout(pollAnalysis, ANALYSIS_POLL_MS);
  }
}

async function loadSavedLap(id) {
  elements.traceDetail.textContent = "저장 랩 불러오는 중";
  try {
    const lap = await getJson(`/api/laps/${encodeURIComponent(id)}`);
    if (state.selectedLapId !== String(id)) return;
    state.displayedTrace = normalizeSavedLap(lap);
    renderTrace(state.displayedTrace, false);
  } catch (error) {
    if (state.selectedLapId !== String(id)) return;
    state.displayedTrace = null;
    renderTrace(null, false);
    elements.traceDetail.textContent = "저장 랩을 불러오지 못했습니다";
  }
}

function renderLive(live) {
  const connected = Boolean(pick(live, ["connected"], false));
  const source = String(pick(live, ["source"], connected ? "LMU" : "연결 대기"));
  const warning = pick(live, ["warning"], "");
  const vehicles = asArray(pick(live, ["vehicles"], []));
  const trackPoints = asArray(pick(live, ["track_points", "trackPoints"], []));
  const contacts = asArray(pick(live, ["recent_contacts", "recentContacts"], []));

  renderConnection(connected, source, warning);
  renderSession(pick(live, ["session"], {}), vehicles, pick(live, ["current_lap", "currentLap"], null));
  renderTrack(trackPoints, vehicles, pick(live, ["player"], null));
  renderLeaderboard(vehicles, pick(live, ["player"], null));
  renderContacts(contacts);
  renderCapture(pick(live, ["capture"], {}));

  elements.lastUpdate.textContent = `마지막 업데이트 ${new Intl.DateTimeFormat("ko-KR", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date())}`;
}

function renderConnection(connected, source, warning = "") {
  elements.connectionBadge.classList.toggle("is-online", connected);
  elements.connectionBadge.classList.toggle("is-offline", !connected && !warning);
  elements.connectionBadge.classList.toggle("is-error", !connected && Boolean(warning));
  setText(elements.connectionLabel, connected ? "CONNECTED" : "OFFLINE");
  setText(elements.sourceLabel, source || "연결 대기");

  const warningText = warning ? String(warning) : "";
  elements.warningBanner.hidden = !warningText;
  setText(elements.warningText, warningText);
}

function renderCapture(capture) {
  const captureState = String(pick(capture, ["state"], "waiting"));
  const operatorPaused = Boolean(pick(capture, ["operator_paused"], false));
  const paused = Boolean(pick(capture, ["paused"], false)) || captureState.toLowerCase().startsWith("paused");
  const persistence = pick(capture, ["persistence"], {});
  const sampleRate = finiteNumber(pick(capture, ["sample_rate_hz"], 0), 0);
  const accepted = finiteNumber(pick(capture, ["accepted_frames"], 0), 0);
  const rejected = finiteNumber(pick(capture, ["rejected_frames"], 0), 0);
  const invalidSession = finiteNumber(pick(capture, ["invalid_session_frames"], 0), 0);
  const duplicates = finiteNumber(pick(capture, ["duplicate_frames"], 0), 0);
  const stalled = finiteNumber(pick(capture, ["stalled_frames"], 0), 0);
  const inconsistent = finiteNumber(pick(capture, ["inconsistent_frames"], 0), 0);
  const telemetryAccepted = finiteNumber(pick(capture, ["telemetry_accepted_samples"], 0), 0);
  const telemetryRejected = finiteNumber(pick(capture, ["telemetry_rejected_samples"], 0), 0);
  const telemetryDuplicates = finiteNumber(pick(capture, ["telemetry_duplicate_samples"], 0), 0);
  const telemetryBackward = finiteNumber(pick(capture, ["telemetry_backward_samples"], 0), 0);
  const telemetryDelayed = finiteNumber(pick(capture, ["telemetry_delayed_samples"], 0), 0);
  const telemetrySudden = finiteNumber(pick(capture, ["telemetry_sudden_change_samples"], 0), 0);
  const age = finiteNumber(pick(capture, ["last_frame_age_ms"]));
  const queued = finiteNumber(pick(persistence, ["queued"], 0), 0);
  const pending = finiteNumber(pick(persistence, ["pending"], 0), 0);
  const written = finiteNumber(pick(persistence, ["written"], 0), 0);
  const failed = finiteNumber(pick(persistence, ["failed"], 0), 0);
  const persistenceError = pick(persistence, ["last_error"], "");
  const quality = pick(capture, ["current_quality"], {});
  const qualityStatus = String(pick(quality, ["status"], "unknown"));
  const reasons = asArray(pick(quality, ["reasons"], []));

  setText(elements.captureState, captureStateLabel(captureState, paused));
  setText(elements.captureRate, `${sampleRate.toFixed(1)} Hz`);
  setText(elements.captureAccepted, formatInteger(accepted));
  setText(elements.captureRejected, formatInteger(rejected));
  setText(elements.captureDuplicates, formatInteger(duplicates));
  setText(elements.captureStalled, formatInteger(stalled));
  setText(elements.captureInconsistent, formatInteger(inconsistent));
  setText(elements.captureInvalidSessions, formatInteger(invalidSession));
  setText(elements.captureTelemetryAccepted, formatInteger(telemetryAccepted));
  setText(elements.captureTelemetryRejected, formatInteger(telemetryRejected));
  setText(elements.captureTelemetryDuplicates, formatInteger(telemetryDuplicates));
  setText(elements.captureTelemetryBackward, formatInteger(telemetryBackward));
  setText(elements.captureTelemetryDelayed, formatInteger(telemetryDelayed));
  setText(elements.captureTelemetrySudden, formatInteger(telemetrySudden));
  setText(elements.captureAge, age == null ? "—" : formatAge(age));
  setText(elements.persistencePending, `${formatInteger(queued)} / ${formatInteger(pending)}`);
  setText(elements.persistenceResult, `${formatInteger(written)} / ${formatInteger(failed)}`);

  const notes = [];
  if (operatorPaused) {
    notes.push("사용자 요청으로 수집이 일시정지되었습니다.");
  } else if (paused) {
    notes.push("게임 세션 시간이 멈춰 있어 새 텔레메트리를 기다리고 있습니다.");
  }
  if (pick(capture, ["session_resumed"], false)) notes.push("이전 실행의 동일 세션을 이어서 수집 중입니다.");
  if (qualityStatus !== "valid" && qualityStatus !== "unknown") {
    notes.push(`현재 랩 품질: ${qualityStatus.toUpperCase()}${reasons.length ? ` · ${reasons.join(", ")}` : ""}`);
  }
  if (persistenceError) notes.push(`저장 오류: ${persistenceError}`);
  if (notes.length === 0) notes.push("수집 프레임과 비동기 저장 큐가 정상적으로 갱신되고 있습니다.");
  setText(elements.captureNote, notes.join(" "));

  elements.captureStrip.classList.toggle("is-paused", paused);
  elements.captureStrip.classList.toggle("is-error", failed > 0 || Boolean(persistenceError));
  elements.captureState.classList.toggle("is-paused", paused);
  elements.captureState.classList.toggle("is-error", failed > 0 || Boolean(persistenceError));
  renderControlButtons(capture);
}

function renderControlButtons(capture) {
  const paused = Boolean(pick(capture, ["operator_paused"], false));
  const locked = state.controlBusy || state.shutdownRequested;
  elements.pauseButton.disabled = locked || paused;
  elements.resumeButton.disabled = locked || !paused;
  elements.shutdownButton.disabled = locked;
}

function renderSession(session, vehicles, currentLap) {
  const trackName = pick(session, ["track_name", "track", "venue", "map_name"], "—");
  const sessionType = pick(session, ["session_type", "type", "name"], "—");
  const phase = pick(session, ["phase", "status", "game_phase"], state.live?.connected ? "LIVE" : "대기 중");
  const remaining = pick(session, ["time_remaining_s", "remaining_time_s", "time_left_s", "time_remaining"], null);
  const lap = pick(currentLap, ["lap_number", "lap", "number"], pick(session, ["current_lap", "lap"], null));
  const totalLapsValue = finiteNumber(pick(session, ["total_laps", "max_laps", "laps"], null));
  const totalLaps = totalLapsValue != null && totalLapsValue > 0 ? totalLapsValue : null;
  const trackTemp = finiteNumber(pick(session, ["track_temp_c", "track_temperature_c", "track_temp"]));
  const airTemp = finiteNumber(pick(session, ["air_temp_c", "ambient_temp_c", "air_temperature_c", "air_temp"]));

  elements.sessionPhase.textContent = formatSessionPhase(phase);
  elements.sessionTrack.textContent = String(trackName);
  elements.sessionType.textContent = String(sessionType);
  elements.sessionRemaining.textContent = formatDuration(remaining);
  elements.sessionLap.textContent = lap == null ? "—" : totalLaps == null ? String(lap) : `${lap} / ${totalLaps}`;
  elements.sessionTemperature.textContent =
    trackTemp == null && airTemp == null
      ? "—"
      : `${formatTemperature(trackTemp)} / ${formatTemperature(airTemp)}`;
  elements.sessionCars.textContent = String(vehicles.length);
}

function vehicleCoordinates(vehicle) {
  const position = pick(vehicle, ["world", "world_position", "coordinates", "location", "point"], vehicle);
  return {
    x: finiteNumber(pick(position, ["x"])),
    z: finiteNumber(pick(position, ["z"])),
  };
}

function renderTrack(rawPoints, vehicles, player) {
  const points = rawPoints
    .map((point) => ({
      x: finiteNumber(point?.x),
      z: finiteNumber(point?.z),
      distance: finiteNumber(pick(point, ["lap_distance_m"])),
    }))
    .filter((point) => point.x != null && point.z != null && point.distance != null)
    .sort((left, right) => left.distance - right.distance);
  const locatedVehicles = vehicles
    .map((vehicle) => ({ vehicle, ...vehicleCoordinates(vehicle) }))
    .filter((entry) => entry.x != null && entry.z != null);

  setText(elements.mapPointCount, points.length);
  setText(elements.mapCarCount, locatedVehicles.length);
  const segments = splitTrackSegments(points);
  const drawableSegments = segments.filter((segment) => segment.length >= 2);
  elements.trackEmpty.hidden = drawableSegments.length > 0;

  if (drawableSegments.length === 0) {
    elements.trackLayer.replaceChildren();
    elements.vehicleLayer.replaceChildren();
    state.trackPathKey = "";
    state.trackProjection = null;
    return;
  }

  const pathKey = points
    .map((point) => `${Math.round(point.distance)}:${Math.round(point.x)}:${Math.round(point.z)}`)
    .join("|");
  if (pathKey !== state.trackPathKey || state.trackProjection == null) {
    const projection = createMapProjection(points);
    const fragment = document.createDocumentFragment();
    for (const segment of drawableSegments) {
      const projectedPoints = segment.map((point) => projection(point));
      if (segments.length === 1 && shouldCloseTrack(points)) projectedPoints.push(projectedPoints[0]);
      const pointString = projectedPoints.map((point) => `${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(" ");
      fragment.append(
        svgElement("polyline", { class: "track-shadow", points: pointString }),
        svgElement("polyline", { class: "track-line", points: pointString }),
        svgElement("polyline", { class: "track-centerline", points: pointString }),
      );
    }
    elements.trackLayer.replaceChildren(fragment);
    state.trackPathKey = pathKey;
    state.trackProjection = projection;
  }

  const projection = state.trackProjection;
  const vehicleFragment = document.createDocumentFragment();
  for (const entry of locatedVehicles) {
    const point = projection(entry);
    const position = racePosition(entry.vehicle);
    const playerVehicle = isPlayerVehicle(entry.vehicle, player);
    const inPit = vehicleInPit(entry.vehicle);
    const group = svgElement("g", {
      class: [
        "vehicle-marker",
        playerVehicle ? "is-player" : "",
        position === 1 ? "is-leader" : "",
        inPit ? "is-pit" : "",
      ]
        .filter(Boolean)
        .join(" "),
      transform: `translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})`,
    });
    const driver = vehicleName(entry.vehicle);
    const title = svgElement("title");
    title.textContent = `${position == null ? "—" : `P${position}`} ${driver}${inPit ? " · PIT" : ""}`;
    const circle = svgElement("circle", { r: playerVehicle ? 12 : 9 });
    group.append(title, circle);
    if (position != null) {
      const label = svgElement("text", { x: 0, y: playerVehicle ? -18 : -15 });
      label.textContent = String(position);
      group.append(label);
    }
    vehicleFragment.append(group);
  }
  elements.vehicleLayer.replaceChildren(vehicleFragment);
}

function splitTrackSegments(points) {
  const segments = [];
  let current = [];
  for (const point of points) {
    const previous = current[current.length - 1];
    if (previous && point.distance - previous.distance > TRACK_MAX_GAP_M) {
      segments.push(current);
      current = [];
    }
    current.push(point);
  }
  if (current.length > 0) segments.push(current);
  return segments;
}

function createMapProjection(points) {
  const xs = points.map((point) => point.x);
  const zs = points.map((point) => point.z);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minZ = Math.min(...zs);
  const maxZ = Math.max(...zs);
  const width = Math.max(maxX - minX, 1);
  const height = Math.max(maxZ - minZ, 1);
  const scale = Math.min((MAP_WIDTH - MAP_PADDING * 2) / width, (MAP_HEIGHT - MAP_PADDING * 2) / height);
  const drawnWidth = width * scale;
  const drawnHeight = height * scale;
  const offsetX = (MAP_WIDTH - drawnWidth) / 2;
  const offsetY = (MAP_HEIGHT - drawnHeight) / 2;

  return (point) => ({
    x: offsetX + (point.x - minX) * scale,
    y: offsetY + (maxZ - point.z) * scale,
  });
}

function shouldCloseTrack(points) {
  const trackLength = finiteNumber(pick(state.live?.session, ["track_length_m"]));
  if (trackLength == null || trackLength <= 0 || points.length < 20) return false;
  const first = points[0];
  const last = points[points.length - 1];
  const wrapGap = first.distance + trackLength - last.distance;
  const worldGap = Math.hypot(first.x - last.x, first.z - last.z);
  return wrapGap <= TRACK_MAX_GAP_M && worldGap <= Math.max(100, trackLength * 0.01);
}

function svgElement(name, attributes = {}) {
  const element = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, String(value));
  return element;
}

function renderLeaderboard(vehicles, player) {
  const sorted = vehicles
    .map((vehicle, index) => ({ vehicle, index }))
    .sort((left, right) => {
      const leftPosition = racePosition(left.vehicle);
      const rightPosition = racePosition(right.vehicle);
      return (leftPosition ?? Number.POSITIVE_INFINITY) - (rightPosition ?? Number.POSITIVE_INFINITY) || left.index - right.index;
    });

  const fragment = document.createDocumentFragment();
  setText(elements.leaderboardCount, sorted.length);
  elements.leaderboardEmpty.hidden = sorted.length > 0;

  for (const { vehicle, index } of sorted) {
    const position = racePosition(vehicle);
    const row = document.createElement("tr");
    row.classList.toggle("is-player", isPlayerVehicle(vehicle, player));

    const positionCell = tableCell(position == null ? "—" : String(position), "position-cell");
    const driverCell = document.createElement("td");
    driverCell.className = "driver-cell";
    const driverName = document.createElement("span");
    driverName.className = "driver-name";
    driverName.textContent = vehicleName(vehicle);
    const driverMeta = document.createElement("span");
    driverMeta.className = "driver-meta";
    const carNumber = pick(vehicle, ["car_number", "number"], null);
    const completedLaps = pick(vehicle, ["completed_laps", "laps_completed", "lap"], null);
    driverMeta.textContent = [carNumber == null ? null : `#${carNumber}`, completedLaps == null ? null : `LAP ${completedLaps}`]
      .filter(Boolean)
      .join(" · ");
    driverCell.append(driverName, driverMeta);

    const classCell = document.createElement("td");
    const classTag = document.createElement("span");
    classTag.className = "class-tag";
    classTag.textContent = String(pick(vehicle, ["class_name", "car_class", "class"], "—"));
    classCell.append(classTag);

    const intervalCell = tableCell(
      formatVehicleGap(vehicle, "interval", position === 1),
      "numeric gap-cell",
    );
    const gapCell = tableCell(
      formatVehicleGap(vehicle, "leader", position === 1),
      "numeric gap-cell",
    );
    const pitCell = document.createElement("td");
    pitCell.className = "col-pit";
    if (vehicleInPit(vehicle)) {
      const badge = document.createElement("span");
      badge.className = "pit-badge";
      badge.textContent = "PIT";
      pitCell.append(badge);
    } else {
      pitCell.textContent = "—";
    }

    row.append(positionCell, driverCell, classCell, intervalCell, gapCell, pitCell);
    fragment.append(row);
  }
  elements.leaderboardBody.replaceChildren(fragment);
}

function racePosition(vehicle) {
  const position = finiteNumber(pick(vehicle, ["position", "place", "rank"]));
  return position != null && position > 0 ? position : null;
}

function tableCell(value, className = "") {
  const cell = document.createElement("td");
  cell.className = className;
  if (value === "LEADER") {
    const leader = document.createElement("span");
    leader.className = "leader-label";
    leader.textContent = value;
    cell.append(leader);
  } else {
    cell.textContent = value;
  }
  return cell;
}

function renderContacts(contacts) {
  const key = contacts.map((contact) => String(pick(contact, ["id"], ""))).join("|");
  setText(elements.contactsCount, contacts.length);
  elements.contactsEmpty.hidden = contacts.length > 0;
  if (key === state.contactsKey) return;
  state.contactsKey = key;
  const fragment = document.createDocumentFragment();

  for (const contact of contacts) {
    const row = document.createElement("article");
    row.className = "contact-row";
    const pair = document.createElement("div");
    pair.className = "contact-pair";
    const driverA = document.createElement("span");
    driverA.textContent = contactDriver(contact, "a");
    const arrow = document.createElement("span");
    arrow.className = "contact-arrow";
    arrow.textContent = "↔";
    const driverB = document.createElement("span");
    driverB.textContent = contactDriver(contact, "b");
    pair.append(driverA, arrow, driverB);

    const timing = document.createElement("div");
    timing.className = "contact-time";
    const lap = pick(contact, ["lap", "lap_number"], pick(contact?.car_a, ["lap_number"], null));
    const time = pick(contact, ["time_ms", "session_time_ms", "time_s", "session_time_s", "time"], null);
    timing.textContent = `${lap == null ? "LAP —" : `LAP ${lap}`} · ${formatContactTime(time, contact)}`;

    const metrics = document.createElement("div");
    metrics.className = "contact-metrics";
    const magnitudeA = finiteNumber(pick(contact, ["magnitude_a", "magnitude", "impact", "strength"]));
    const magnitudeB = finiteNumber(pick(contact, ["magnitude_b"]));
    const magnitude = magnitudeA == null ? magnitudeB : magnitudeB == null ? magnitudeA : Math.max(magnitudeA, magnitudeB);
    const magnitudeLabel = document.createElement("span");
    magnitudeLabel.className = "contact-magnitude";
    magnitudeLabel.textContent = magnitude == null ? "IMPACT —" : `IMPACT ${formatCompactNumber(magnitude)}`;

    const confidenceValue = normalizeConfidence(pick(contact, ["confidence", "confidence_score"], 0));
    const confidence = document.createElement("span");
    confidence.className = "confidence-meter";
    const confidenceText = document.createElement("span");
    confidenceText.textContent = `신뢰도 ${formatConfidenceLabel(pick(contact, ["confidence", "confidence_score"], 0), confidenceValue)}`;
    const confidenceTrack = document.createElement("span");
    confidenceTrack.className = "confidence-track";
    const confidenceFill = document.createElement("i");
    confidenceFill.className = "confidence-fill";
    confidenceFill.style.width = `${confidenceValue * 100}%`;
    confidenceTrack.append(confidenceFill);
    confidence.append(confidenceText, confidenceTrack);
    metrics.append(magnitudeLabel, confidence);

    row.append(pair, timing, metrics);
    fragment.append(row);
  }
  elements.contactsList.replaceChildren(fragment);
}

function renderAnalysis(report) {
  const status = String(pick(report, ["status"], "waiting"));
  const cohort = pick(report, ["cohort"], {});
  const generatedAt = finiteNumber(pick(report, ["generated_at_unix_ms"]));
  const focusZones = asArray(pick(report, ["focus_zones"], []));
  const segments = asArray(pick(report, ["segments"], []));
  const exclusions = asArray(pick(report, ["exclusions"], []));
  const limitations = asArray(pick(report, ["limitations"], []));

  setText(elements.analysisStatus, analysisStatusLabel(status));
  setText(elements.analysisMessage, pick(report, ["message"], "분석 데이터를 기다리고 있습니다."));
  setText(
    elements.analysisGenerated,
    generatedAt && generatedAt > 0
      ? `생성 ${new Intl.DateTimeFormat("ko-KR", {
          month: "2-digit",
          day: "2-digit",
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        }).format(new Date(generatedAt))}`
      : "아직 생성되지 않음",
  );
  setText(elements.analysisTrack, pick(report, ["track_name"], "—") || "—");
  setText(elements.analysisSession, pick(report, ["session_type"], "—") || "—");
  setText(elements.analysisClass, pick(report, ["class_name"], "—") || "—");
  elements.analysisStatus.classList.toggle("is-ready", status === "ready");
  elements.analysisStatus.classList.toggle("is-warning", status !== "ready" && status !== "unavailable");
  elements.analysisStatus.classList.toggle("is-error", status === "unavailable" || status === "error");

  setText(elements.cohortParticipants, formatInteger(pick(cohort, ["participant_count"], 0)));
  setText(elements.cohortValidLaps, formatInteger(pick(cohort, ["valid_lap_count"], 0)));
  setText(elements.cohortTopCount, formatInteger(pick(cohort, ["top_quartile_count"], 0)));
  const topMedian = finiteNumber(pick(cohort, ["top_quartile_median_ms"]));
  setText(elements.cohortTopMedian, topMedian == null ? "중앙값 —" : `중앙값 ${formatLapTime(topMedian)}`);

  renderBenchmark(elements.playerBenchmark, pick(report, ["player"], null), "내 유효 기록을 기다리고 있습니다.");
  renderBenchmark(elements.p1Benchmark, pick(report, ["actual_p1"], null), "실제 클래스 P1 텔레메트리가 없습니다.");
  renderBenchmark(elements.fastestBenchmark, pick(report, ["fastest_captured"], null), "수집된 유효 기록이 없습니다.");
  renderFocusZones(focusZones);
  renderSegments(segments);
  renderExclusions(exclusions);
  renderLimitations(limitations);
}

function renderBenchmark(container, benchmark, emptyMessage) {
  if (!benchmark || typeof benchmark !== "object") {
    const empty = document.createElement("p");
    empty.className = "benchmark-empty";
    empty.textContent = emptyMessage;
    container.replaceChildren(empty);
    return;
  }

  const header = document.createElement("div");
  header.className = "benchmark-driver";
  const name = document.createElement("strong");
  name.textContent = String(pick(benchmark, ["driver_name"], "Unknown"));
  const rank = document.createElement("span");
  const rankValue = finiteNumber(pick(benchmark, ["rank"]));
  const percentile = finiteNumber(pick(benchmark, ["percentile"]));
  rank.textContent = [rankValue == null ? null : `P${rankValue}`, percentile == null ? null : `백분위 ${formatPercentile(percentile)}`]
    .filter(Boolean)
    .join(" · ");
  header.append(name, rank);

  const times = document.createElement("dl");
  times.className = "benchmark-times";
  times.append(
    definition("Best", formatLapTime(pick(benchmark, ["best_lap_ms"], null))),
    definition("Best 2 중앙값", formatLapTime(pick(benchmark, ["best_two_median_ms"], null))),
    definition(
      "선택 / 유효 랩",
      `${formatInteger(pick(benchmark, ["selected_lap_count"], 0))} / ${formatInteger(pick(benchmark, ["valid_lap_count"], 0))}`,
    ),
  );
  container.replaceChildren(header, times);
}

function definition(term, description) {
  const wrapper = document.createElement("div");
  const dt = document.createElement("dt");
  const dd = document.createElement("dd");
  dt.textContent = term;
  dd.textContent = description;
  wrapper.append(dt, dd);
  return wrapper;
}

function renderFocusZones(zones) {
  setText(elements.focusCount, zones.length);
  elements.focusEmpty.hidden = zones.length > 0;
  const fragment = document.createDocumentFragment();
  for (const zone of zones) {
    const card = document.createElement("article");
    card.className = "focus-card";

    const rank = document.createElement("span");
    rank.className = "focus-rank";
    rank.textContent = String(pick(zone, ["rank"], "—"));
    const main = document.createElement("div");
    main.className = "focus-main";
    const heading = document.createElement("div");
    heading.className = "focus-heading";
    const title = document.createElement("strong");
    title.textContent = `구간 ${pick(zone, ["segment_number"], "—")}`;
    const range = document.createElement("span");
    range.textContent = formatDistanceRange(zone);
    heading.append(title, range);
    const tags = document.createElement("div");
    tags.className = "analysis-tags";
    tags.append(
      analysisTag(patternLabel(pick(zone, ["pattern"], "unknown"))),
      analysisTag(lossOriginLabel(pick(zone, ["loss_origin"], "unknown"))),
      analysisTag(`신뢰도 ${confidenceLabel(pick(zone, ["confidence"], "unknown"))}`),
    );
    const cues = document.createElement("ul");
    cues.className = "coaching-cues";
    for (const cue of asArray(pick(zone, ["coaching_cues"], []))) {
      const item = document.createElement("li");
      item.textContent = String(cue);
      cues.append(item);
    }
    main.append(heading, tags, cues);

    const loss = document.createElement("strong");
    loss.className = "focus-loss";
    loss.textContent = formatLoss(pick(zone, ["loss_ms"], 0));
    card.append(rank, main, loss);
    fragment.append(card);
  }
  elements.focusZones.replaceChildren(fragment);
}

function renderSegments(segments) {
  setText(elements.segmentsCount, segments.length);
  elements.segmentsEmpty.hidden = segments.length > 0;
  const fragment = document.createDocumentFragment();
  for (const segment of segments) {
    const row = document.createElement("tr");
    const loss = finiteNumber(pick(segment, ["delta_to_actual_p1_ms"]));
    if (loss > 0) row.classList.add("is-loss");
    if (loss < 0) row.classList.add("is-gain");
    row.append(
      tableCell(`S${pick(segment, ["number"], "—")}`, "segment-number"),
      tableCell(formatDistanceRange(segment), "segment-range"),
      tableCell(formatSegmentTime(pick(segment, ["player_time_ms"], null)), "numeric"),
      tableCell(formatSegmentTime(pick(segment, ["actual_p1_time_ms"], null)), "numeric"),
      tableCell(loss == null ? "—" : formatLoss(loss), "numeric segment-loss"),
      tableCell(
        pick(segment, ["cumulative_delta_to_actual_p1_ms"], null) == null
          ? "—"
          : formatLoss(pick(segment, ["cumulative_delta_to_actual_p1_ms"], 0)),
        "numeric",
      ),
      tableCell(
        `${pick(segment, ["rank"], "—")} / ${pick(segment, ["participant_count"], "—")} · ${formatPercentile(finiteNumber(pick(segment, ["percentile"]), 0))}`,
        "numeric",
      ),
      tableCell(`${patternLabel(pick(segment, ["pattern"], "unknown"))} · ${lossOriginLabel(pick(segment, ["loss_origin"], "unknown"))}`),
      tableCell(
        `${confidenceLabel(pick(segment, ["confidence"], "unknown"))} · n=${formatInteger(pick(segment, ["lap_sample_count"], 0))}`,
        "confidence-cell",
      ),
    );
    fragment.append(row);
  }
  elements.segmentsBody.replaceChildren(fragment);
}

function renderExclusions(exclusions) {
  elements.exclusionsEmpty.hidden = exclusions.length > 0;
  const fragment = document.createDocumentFragment();
  for (const exclusion of exclusions) {
    const row = document.createElement("article");
    row.className = "quality-row";
    const copy = document.createElement("div");
    const code = document.createElement("strong");
    code.textContent = String(pick(exclusion, ["code"], "unknown"));
    const description = document.createElement("p");
    description.textContent = String(pick(exclusion, ["description"], "제외 사유 정보 없음"));
    copy.append(code, description);
    const count = document.createElement("span");
    count.textContent = `${formatInteger(pick(exclusion, ["count"], 0))}건`;
    row.append(copy, count);
    fragment.append(row);
  }
  elements.exclusionsList.replaceChildren(fragment);
}

function renderLimitations(limitations) {
  elements.limitationsEmpty.hidden = limitations.length > 0;
  const fragment = document.createDocumentFragment();
  for (const limitation of limitations) {
    const item = document.createElement("li");
    item.textContent = String(limitation);
    fragment.append(item);
  }
  elements.limitationsList.replaceChildren(fragment);
}

function analysisTag(label) {
  const tag = document.createElement("span");
  tag.textContent = label;
  return tag;
}

function renderLapOptions() {
  const selected = state.selectedLapId;
  const optionsKey = state.savedLaps
    .map((lap) =>
      [
        pick(lap, ["id", "lap_id", "key"], ""),
        pick(lap, ["lap_time_ms"], ""),
        pick(lap, ["valid"], ""),
        pick(lap, ["vehicle_id", "vehicleId"], ""),
        pick(lap, ["driver_name", "driverName"], ""),
        pick(lap, ["class_name", "className"], ""),
        pick(lap, ["is_player", "isPlayer"], ""),
      ].join(":"),
    )
    .join("|");
  if (optionsKey === state.lapOptionsKey) return;
  state.lapOptionsKey = optionsKey;
  const fragment = document.createDocumentFragment();
  const liveOption = document.createElement("option");
  liveOption.value = "live";
  liveOption.textContent = "현재 랩 · 내 차 · LIVE";
  fragment.append(liveOption);

  const knownIds = new Set(["live"]);
  for (const lap of state.savedLaps) {
    const id = String(pick(lap, ["id", "lap_id", "key"], ""));
    if (!id || knownIds.has(id)) continue;
    knownIds.add(id);
    const option = document.createElement("option");
    option.value = id;
    const lapNumber = pick(lap, ["lap_number", "lap", "number"], "—");
    const lapTime = pick(lap, ["lap_time_ms", "time_ms", "lap_time"], null);
    const valid = pick(lap, ["valid", "is_valid", "clean"], true);
    const identity = lapIdentity(lap);
    option.textContent = `${identity ? `${identity} · ` : ""}랩 ${lapNumber} · ${formatLapTime(lapTime)}${valid === false ? " · INVALID" : ""}`;
    fragment.append(option);
  }

  elements.lapSelect.replaceChildren(fragment);
  if (knownIds.has(selected)) {
    elements.lapSelect.value = selected;
  } else {
    state.selectedLapId = "live";
    state.displayedTrace = state.liveTrace;
    elements.lapSelect.value = "live";
    renderTrace(state.displayedTrace, true);
  }
}

function lapIdentity(lap) {
  const driverName = String(pick(lap, ["driver_name", "driverName"], "")).trim();
  const className = String(pick(lap, ["class_name", "className"], "")).trim();
  const vehicleId = finiteNumber(pick(lap, ["vehicle_id", "vehicleId"]));
  const isPlayer = pick(lap, ["is_player", "isPlayer"], false) === true;
  return [
    isPlayer ? "내 차" : null,
    driverName || (vehicleId == null ? null : `차량 ${vehicleId}`),
    className || null,
  ]
    .filter(Boolean)
    .join(" · ");
}

function normalizeSavedLap(lap) {
  if (!lap || typeof lap !== "object") return null;
  if (lap.summary || lap.samples) return lap;
  return {
    summary: lap,
    samples: pick(lap, ["trace", "telemetry"], []),
  };
}

function renderTrace(trace, live) {
  const summary = pick(trace, ["summary"], trace || {});
  const samples = asArray(pick(trace, ["samples", "trace", "telemetry"], []));
  const currentLap = live ? pick(state.live, ["current_lap", "currentLap"], {}) : {};
  const lapNumber = pick(summary, ["lap_number", "lap", "number"], pick(currentLap, ["lap_number"], null));
  const summaryTime = pick(summary, ["lap_time_ms", "current_lap_time_ms", "time_ms", "lap_time"], null);
  const elapsedSeconds = finiteNumber(pick(currentLap, ["lap_elapsed_s"]));
  const lapTime = summaryTime ?? (elapsedSeconds == null ? null : elapsedSeconds * 1000);
  const valid = pick(summary, ["valid", "is_valid", "clean"], !pick(currentLap, ["invalid"], false));
  const sampleCount = finiteNumber(
    pick(summary, ["sample_count", "samples"], pick(currentLap, ["sample_count"], samples.length)),
    samples.length,
  );
  const maxSpeed = finiteNumber(pick(summary, ["max_speed_kmh", "top_speed_kmh", "max_speed"]));
  const identity = live ? "내 차" : lapIdentity(summary);

  elements.traceLap.textContent = live
    ? lapNumber == null
      ? "현재 랩"
      : `현재 랩 ${lapNumber}`
    : lapNumber == null
      ? "저장 랩"
      : `저장 랩 ${lapNumber}`;
  elements.traceTime.textContent = formatLapTime(lapTime);
  elements.traceDetail.textContent = [
    identity || null,
    `${sampleCount} samples`,
    maxSpeed == null ? null : `최고 ${Math.round(maxSpeed)} km/h`,
    valid === false ? "INVALID" : null,
  ]
    .filter(Boolean)
    .join(" · ");
  elements.chartEmpty.hidden = samples.length > 1;
  drawTelemetryChart(samples);
}

function drawTelemetryChart(samples) {
  const canvas = elements.chart;
  const rect = canvas.getBoundingClientRect();
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const width = Math.max(Math.round(rect.width), 1);
  const height = Math.max(Math.round(rect.height), 1);
  const pixelWidth = Math.round(width * ratio);
  const pixelHeight = Math.round(height * ratio);
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth;
    canvas.height = pixelHeight;
  }

  chartContext.setTransform(ratio, 0, 0, ratio, 0, 0);
  chartContext.clearRect(0, 0, width, height);

  if (samples.length < 2) return;

  const plot = { left: width < 600 ? 50 : 62, top: 12, right: 12, bottom: 25 };
  const plotWidth = Math.max(width - plot.left - plot.right, 1);
  const plotHeight = Math.max(height - plot.top - plot.bottom, 1);
  const lanes = [
    { key: ["speed_kmh", "speed"], label: "KM/H", color: "#39c8ff", min: 0, max: seriesMax(samples, ["speed_kmh", "speed"], 300, 50) },
    { key: ["throttle"], label: "THR", color: "#80e35a", min: 0, max: 1, normalize: normalizePedal },
    { key: ["brake"], label: "BRK", color: "#ff4c55", min: 0, max: 1, normalize: normalizePedal },
    { key: ["steer", "steering"], label: "STR", color: "#f4c94a", min: -1, max: 1, normalize: normalizeSteer },
    { key: ["rpm"], label: "RPM", color: "#b88cff", min: 0, max: seriesMax(samples, ["rpm"], 9000, 1000) },
  ];
  const laneHeight = plotHeight / lanes.length;
  const progressValues = sampleProgress(samples);

  chartContext.lineWidth = 1;
  chartContext.strokeStyle = "#243039";
  chartContext.fillStyle = "#7f8d95";
  chartContext.font = "9px ui-sans-serif, system-ui, sans-serif";
  chartContext.textAlign = "right";
  chartContext.textBaseline = "middle";

  for (let index = 0; index <= lanes.length; index += 1) {
    const y = plot.top + laneHeight * index;
    chartContext.beginPath();
    chartContext.moveTo(plot.left, y);
    chartContext.lineTo(width - plot.right, y);
    chartContext.stroke();
  }

  for (let index = 0; index <= 4; index += 1) {
    const x = plot.left + (plotWidth * index) / 4;
    chartContext.strokeStyle = index === 0 || index === 4 ? "#2b3740" : "#182229";
    chartContext.beginPath();
    chartContext.moveTo(x, plot.top);
    chartContext.lineTo(x, plot.top + plotHeight);
    chartContext.stroke();
    chartContext.fillStyle = "#64727a";
    chartContext.textAlign = "center";
    chartContext.textBaseline = "top";
    chartContext.fillText(`${index * 25}%`, x, plot.top + plotHeight + 8);
  }

  lanes.forEach((lane, laneIndex) => {
    const top = plot.top + laneHeight * laneIndex + 6;
    const usableHeight = Math.max(laneHeight - 12, 1);
    chartContext.fillStyle = lane.color;
    chartContext.textAlign = "right";
    chartContext.textBaseline = "middle";
    chartContext.fillText(lane.label, plot.left - 9, top + usableHeight / 2);

    if (lane.min < 0 && lane.max > 0) {
      const zeroY = top + usableHeight * (lane.max / (lane.max - lane.min));
      chartContext.strokeStyle = "#36434b";
      chartContext.beginPath();
      chartContext.moveTo(plot.left, zeroY);
      chartContext.lineTo(width - plot.right, zeroY);
      chartContext.stroke();
    }

    chartContext.strokeStyle = lane.color;
    chartContext.lineWidth = 1.6;
    chartContext.lineJoin = "round";
    chartContext.lineCap = "round";
    chartContext.beginPath();
    let started = false;
    samples.forEach((sample, sampleIndex) => {
      let value = finiteNumber(pick(sample, lane.key));
      if (value == null) return;
      if (lane.normalize) value = lane.normalize(value);
      const normalized = Math.max(0, Math.min(1, (value - lane.min) / (lane.max - lane.min || 1)));
      const x = plot.left + progressValues[sampleIndex] * plotWidth;
      const y = top + usableHeight * (1 - normalized);
      if (!started) {
        chartContext.moveTo(x, y);
        started = true;
      } else {
        chartContext.lineTo(x, y);
      }
    });
    chartContext.stroke();
  });
}

function sampleProgress(samples) {
  const distances = samples.map((sample) =>
    finiteNumber(pick(sample, ["lap_distance_m", "distance_m", "lap_distance", "distance"])),
  );
  const hasDistanceRange = distances.every((value) => value != null) && Math.max(...distances) > Math.min(...distances);
  if (hasDistanceRange) {
    const min = Math.min(...distances);
    const range = Math.max(...distances) - min;
    return distances.map((value) => (value - min) / range);
  }

  const times = samples.map((sample) =>
    finiteNumber(pick(sample, ["lap_time_ms", "time_ms", "elapsed_ms", "lap_elapsed_s", "time_s", "session_time_s", "session_time"])),
  );
  const hasTimeRange = times.every((value) => value != null) && Math.max(...times) > Math.min(...times);
  if (hasTimeRange) {
    const min = Math.min(...times);
    const range = Math.max(...times) - min;
    return times.map((value) => (value - min) / range);
  }
  return samples.map((_, index) => index / Math.max(samples.length - 1, 1));
}

function seriesMax(samples, keys, fallback, rounding) {
  const max = Math.max(0, ...samples.map((sample) => finiteNumber(pick(sample, keys), 0)));
  if (max <= 0) return fallback;
  return Math.max(rounding, Math.ceil(max / rounding) * rounding);
}

function normalizePedal(value) {
  return value > 1 ? Math.min(value / 100, 1) : Math.max(0, value);
}

function normalizeSteer(value) {
  if (Math.abs(value) > 1) return Math.max(-1, Math.min(1, value / 100));
  return Math.max(-1, Math.min(1, value));
}

function vehicleName(vehicle) {
  return String(pick(vehicle, ["driver_name", "name", "driver", "player_name"], "Unknown"));
}

function vehicleIdentity(vehicle) {
  return pick(vehicle, ["id", "vehicle_id", "index", "slot_id", "car_id"], null);
}

function isPlayerVehicle(vehicle, player) {
  if (pick(vehicle, ["is_player", "player"], false) === true) return true;
  if (player == null) return false;
  const vehicleId = vehicleIdentity(vehicle);
  const playerId = typeof player === "object" ? vehicleIdentity(player) : player;
  if (vehicleId != null && playerId != null && String(vehicleId) === String(playerId)) return true;
  if (typeof player === "object") {
    const playerVehicleId = pick(player, ["vehicle_id", "car_id", "player_vehicle_id"], null);
    if (vehicleId != null && playerVehicleId != null && String(vehicleId) === String(playerVehicleId)) return true;
  }
  return false;
}

function vehicleInPit(vehicle) {
  return Boolean(pick(vehicle, ["in_pits", "in_pit", "is_in_pit", "pit", "pit_status"], false));
}

function contactDriver(contact, side) {
  const nested = pick(contact, [side, `car_${side}`, `vehicle_${side}`, `driver_${side}`], null);
  if (nested && typeof nested === "object") return vehicleName(nested);
  return String(
    pick(
      contact,
      [`${side}_name`, `driver_${side}_name`, `vehicle_${side}_name`, `${side}_driver`],
      nested == null ? "Unknown" : nested,
    ),
  );
}

function normalizeConfidence(value) {
  if (typeof value === "string") {
    const normalized = value.toLowerCase();
    if (normalized === "confirmed") return 1;
    if (normalized === "probable") return 0.7;
    if (normalized === "unresolved") return 0.35;
  }
  const confidence = finiteNumber(value, 0);
  return Math.max(0, Math.min(1, confidence > 1 ? confidence / 100 : confidence));
}

function formatConfidenceLabel(value, normalizedValue) {
  if (typeof value === "string") return value.toUpperCase();
  return `${Math.round(normalizedValue * 100)}%`;
}

function formatSessionPhase(value) {
  if (typeof value === "number" || /^\d+$/.test(String(value))) return `PHASE ${value}`;
  return String(value);
}

function captureStateLabel(value, paused) {
  if (paused) return "일시정지";
  const labels = {
    active: "수집 중",
    capturing: "수집 중",
    connected: "연결됨",
    disconnected: "연결 대기",
    waiting: "대기",
    stalled: "데이터 정체",
    error: "오류",
  };
  const key = String(value).toLowerCase();
  return labels[key] || String(value || "대기").toUpperCase();
}

function analysisStatusLabel(status) {
  const labels = {
    ready: "READY",
    waiting_for_session: "세션 대기",
    waiting_for_qualifying_session: "퀄리파잉 대기",
    waiting_for_player_lap: "내 랩 대기",
    waiting_for_valid_laps: "유효 랩 대기",
    waiting_for_valid_player_trace: "내 유효 트레이스 대기",
    waiting_for_actual_p1_trace: "P1 트레이스 대기",
    unavailable: "UNAVAILABLE",
    error: "ERROR",
  };
  return labels[status] || String(status || "WAITING").replaceAll("_", " ").toUpperCase();
}

function patternLabel(value) {
  const labels = {
    recurring: "반복 습관",
    one_off: "일회성",
    isolated: "일회성",
    none: "뚜렷한 반복 없음",
    unknown: "판정 대기",
  };
  return labels[String(value)] || String(value).replaceAll("_", " ");
}

function lossOriginLabel(value) {
  const labels = {
    carry: "이전 구간 영향",
    intrinsic: "구간 자체 손실",
    mixed: "복합 손실",
    none: "손실 없음",
    unknown: "원인 대기",
  };
  return labels[String(value)] || String(value).replaceAll("_", " ");
}

function confidenceLabel(value) {
  const labels = { high: "높음", medium: "보통", low: "낮음", unknown: "판정 대기" };
  return labels[String(value)] || String(value).toUpperCase();
}

function formatAge(milliseconds) {
  const value = Math.max(0, milliseconds);
  if (value < 1000) return `${Math.round(value)} ms`;
  if (value < 60000) return `${(value / 1000).toFixed(1)} s`;
  return `${Math.floor(value / 60000)}m ${Math.floor((value % 60000) / 1000)}s`;
}

function formatInteger(value) {
  const number = finiteNumber(value, 0);
  return new Intl.NumberFormat("ko-KR", { maximumFractionDigits: 0 }).format(number);
}

function formatPercentile(value) {
  return `${Math.max(0, Math.min(100, value)).toFixed(1)}%`;
}

function formatDistanceRange(value) {
  const start = finiteNumber(pick(value, ["start_m"]));
  const end = finiteNumber(pick(value, ["end_m"]));
  return start == null || end == null ? "거리 —" : `${Math.round(start)}–${Math.round(end)} m`;
}

function formatSegmentTime(value) {
  const milliseconds = finiteNumber(value);
  return milliseconds == null ? "—" : `${(milliseconds / 1000).toFixed(3)} s`;
}

function formatLoss(value) {
  const milliseconds = finiteNumber(value);
  if (milliseconds == null) return "—";
  const sign = milliseconds > 0 ? "+" : milliseconds < 0 ? "−" : "±";
  return `${sign}${(Math.abs(milliseconds) / 1000).toFixed(3)} s`;
}

function formatTemperature(value) {
  return value == null ? "—" : `${Math.round(value)}°C`;
}

function formatDuration(value) {
  if (value == null || value === "") return "—";
  if (typeof value === "string" && !/^\d+(\.\d+)?$/.test(value)) return value;
  const seconds = Math.max(0, Math.round(finiteNumber(value, 0)));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = seconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remainingSeconds).padStart(2, "0")}`
    : `${minutes}:${String(remainingSeconds).padStart(2, "0")}`;
}

function formatLapTime(value) {
  if (value == null || value === "") return "—:—.—";
  if (typeof value === "string" && value.includes(":")) return value;
  const numeric = finiteNumber(value);
  if (numeric == null || numeric < 0) return "—:—.—";
  const milliseconds = numeric < 1000 && !Number.isInteger(numeric) ? numeric * 1000 : numeric;
  const minutes = Math.floor(milliseconds / 60000);
  const seconds = Math.floor((milliseconds % 60000) / 1000);
  const millis = Math.floor(milliseconds % 1000);
  return `${minutes}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

function formatRaceGap(value, leader = false, seconds = false) {
  if (leader) return "LEADER";
  if (value == null || value === "") return "—";
  if (typeof value === "string") {
    if (/lap/i.test(value)) return value.toUpperCase();
    const numericString = Number(value);
    if (!Number.isFinite(numericString)) return value;
  }
  const numeric = finiteNumber(value);
  if (numeric == null) return "—";
  const milliseconds = seconds ? numeric * 1000 : Math.abs(numeric) < 1000 && !Number.isInteger(numeric) ? numeric * 1000 : numeric;
  const sign = milliseconds >= 0 ? "+" : "−";
  const absolute = Math.abs(milliseconds);
  if (absolute >= 60000) {
    const minutes = Math.floor(absolute / 60000);
    const seconds = (absolute % 60000) / 1000;
    return `${sign}${minutes}:${seconds.toFixed(3).padStart(6, "0")}`;
  }
  return `${sign}${(absolute / 1000).toFixed(3)}`;
}

function formatVehicleGap(vehicle, kind, leader) {
  if (leader) return "LEADER";
  const isInterval = kind === "interval";
  const lapsBehind = finiteNumber(
    pick(vehicle, [isInterval ? "laps_behind_next" : "laps_behind_leader"]),
    0,
  );
  if (lapsBehind > 0) return `+${lapsBehind} LAP${lapsBehind === 1 ? "" : "S"}`;
  const secondsValue = pick(vehicle, [isInterval ? "interval_s" : "gap_to_leader_s"], null);
  const millisecondValue = pick(
    vehicle,
    isInterval ? ["interval_ms", "interval", "gap_to_ahead_ms"] : ["gap_to_leader_ms", "leader_gap_ms", "gap"],
    null,
  );
  return formatRaceGap(secondsValue ?? millisecondValue, false, secondsValue != null);
}

function formatContactTime(value, contact) {
  if (value == null || value === "") return "—:—.—";
  if (typeof value === "string" && value.includes(":")) return value;
  let numeric = finiteNumber(value);
  if (numeric == null) return "—:—.—";
  const keyIsSeconds = contact.time_s != null || contact.session_time_s != null;
  if (keyIsSeconds) numeric *= 1000;
  return formatLapTime(numeric);
}

function formatCompactNumber(value) {
  const absolute = Math.abs(value);
  if (absolute >= 1000) return value.toFixed(0);
  if (absolute >= 100) return value.toFixed(1);
  return value.toFixed(2);
}

renderConnection(false, "연결 대기");
renderTrace(null, true);
renderCapture({});
renderAnalysis({ status: "waiting_for_session", message: "LMU 세션 분석을 기다리고 있습니다." });
pollLive();
pollTraceAndLaps();
pollAnalysis();
