--!strict
-- Tern lens for pr-peek.
--
-- pr-peek prints JSON when PR_PEEK_JSON=1 (see --json in src/main.rs). The
-- `spawn` hook below sets that variable in every new shell pane, and the lens
-- decodes the JSON and renders it with Tern's native table/badge widgets. If
-- the output is not JSON (older pr-peek, an error, PR_PEEK_JSON overridden)
-- the lens returns nil and Tern keeps the raw text.

local ui = tern.ui
local json = tern.json

-- Guard against runaway output; past this we give up and show raw text.
local MAX_LINES = 20000
local MAX_BYTES = 4 * 1024 * 1024

type Entry = { position: number?, state: string? }
type Pr = {
	number: number,
	title: string,
	url: string,
	isDraft: boolean?,
	headRefName: string,
	baseRefName: string,
	reviewDecision: string?,
	mergeQueueEntry: Entry?,
}
type Output = { owner: string, repo: string, user: string, pullRequests: { Pr } }

type State = {
	run: LensRun,
	lines: { string },
	bytes: number,
	overflow: boolean,
	status: number?,
	warned: boolean,
}

-- Decoding -------------------------------------------------------------------

local function decode(state: State): Output?
	if state.overflow or #state.lines == 0 then
		return nil
	end
	local text = table.concat(state.lines, "\n")
	-- Cheap pre-check so we don't throw on every plain-text run.
	if string.match(text, "^%s*{") == nil then
		return nil
	end
	local ok, value = pcall(json.decode, text)
	if not ok or type(value) ~= "table" or type(value.pullRequests) ~= "table" then
		return nil
	end
	return value :: Output
end

-- Cells ----------------------------------------------------------------------

local REVIEW: { [string]: { t: string, s: string } } = {
	APPROVED = { t = "✓ approved", s = "success strong" },
	CHANGES_REQUESTED = { t = "✗ changes requested", s = "error strong" },
	REVIEW_REQUIRED = { t = "○ review required", s = "warning strong" },
}

local QUEUE_LABEL: { [string]: string } = {
	AWAITING_CHECKS = "awaiting checks",
	LOCKED = "locked",
	MERGEABLE = "mergeable",
	QUEUED = "queued",
	UNMERGEABLE = "unmergeable",
}

local QUEUE_STYLE: { [string]: string } = {
	MERGEABLE = "success strong",
	UNMERGEABLE = "error strong",
}

local function blank(): Span
	return ui.span("—", "muted")
end

local function reviewCell(pr: Pr): Cell
	local r = pr.reviewDecision and REVIEW[pr.reviewDecision]
	if r == nil then
		return blank()
	end
	return ui.span(r.t, r.s)
end

local function queueCell(pr: Pr): Cell
	local e = pr.mergeQueueEntry
	if e == nil or e.state == nil then
		return blank()
	end
	local state = e.state :: string
	local label = QUEUE_LABEL[state] or string.lower((string.gsub(state, "_", " ")))
	local text = if e.position then string.format("#%d %s", e.position, label) else label
	return ui.span(text, QUEUE_STYLE[state] or "warning strong")
end

local function branchCell(pr: Pr): Cell
	return {
		ui.span(pr.headRefName, "mono"),
		ui.span(" → ", "muted"),
		ui.span(pr.baseRefName, "mono muted"),
	}
end

local function titleCell(pr: Pr): Cell
	if pr.isDraft then
		return { ui.span(pr.title, "muted"), ui.span("  draft", "muted") }
	end
	return ui.span(pr.title, "strong")
end

-- Summary --------------------------------------------------------------------

local function plural(n: number, word: string): string
	return if n == 1 then `{n} {word}` else `{n} {word}s`
end

local function summary(prs: { Pr }): Node
	local approved, changes, required, draft, queued = 0, 0, 0, 0, 0
	for _, pr in prs do
		if pr.reviewDecision == "APPROVED" then
			approved += 1
		elseif pr.reviewDecision == "CHANGES_REQUESTED" then
			changes += 1
		elseif pr.reviewDecision == "REVIEW_REQUIRED" then
			required += 1
		end
		if pr.isDraft then
			draft += 1
		end
		if pr.mergeQueueEntry ~= nil then
			queued += 1
		end
	end

	local chips = { ui.badge(plural(#prs, "open PR"), "accent") }
	if approved > 0 then
		table.insert(chips, ui.badge(`{approved} approved`, "success"))
	end
	if changes > 0 then
		table.insert(chips, ui.badge(`{changes} changes requested`, "error"))
	end
	if required > 0 then
		table.insert(chips, ui.badge(`{required} review required`, "warning"))
	end
	if queued > 0 then
		table.insert(chips, ui.badge(`{queued} in merge queue`, "info"))
	end
	if draft > 0 then
		table.insert(chips, ui.badge(plural(draft, "draft"), "neutral"))
	end
	return ui.row(chips)
end

-- View -----------------------------------------------------------------------

local COLS: { Col } = {
	{ id = "number", head = "#", priority = 10 },
	{ id = "title", head = "Title", priority = 9, grow = 1, truncate = "end" },
	{ id = "review", head = "Review", priority = 8 },
	{ id = "queue", head = "Merge queue", priority = 7 },
	{ id = "branch", head = "Branch", priority = 6, truncate = "middle" },
	{ id = "open", head = "", priority = 5 },
}

local function render(out: Output): Node
	local head: Spans = {
		ui.span("Open PRs from ", "muted"),
		ui.span(out.user, "strong"),
		ui.span(" in ", "muted"),
		ui.span(`{out.owner}/{out.repo}`, "strong"),
	}

	local prs = out.pullRequests
	if #prs == 0 then
		return ui.card(head, { ui.text(ui.span("No open pull requests.", "muted")) })
	end

	local rows: { { Cell } } = {}
	for _, pr in prs do
		table.insert(rows, {
			ui.link(`#{pr.number}`, pr.url, "strong"),
			titleCell(pr),
			reviewCell(pr),
			queueCell(pr),
			branchCell(pr),
			ui.link("open ↗", pr.url),
		})
	end

	return ui.card(head, { summary(prs), ui.table(COLS, rows) })
end

-- Lens -----------------------------------------------------------------------

-- open/line/finish only record: Tern replays them on rehydration, so they
-- must stay side-effect free.
tern.lens.define("pr-peek", {
	open = function(run: LensRun, _cx: EffectCx): State
		return { run = run, lines = {}, bytes = 0, overflow = false, status = nil, warned = false }
	end,

	line = function(state: State, l: LensLine)
		if state.overflow then
			return
		end
		state.bytes += #l.text + 1
		if #state.lines >= MAX_LINES or state.bytes > MAX_BYTES then
			state.overflow = true
			state.lines = {}
			return
		end
		table.insert(state.lines, l.text)
	end,

	finish = function(state: State, status: number)
		state.status = status
	end,

	view = function(state: State): Node?
		-- Partial JSON (command still running) fails to decode and falls
		-- through to the raw view; the closing brace arrives with finish.
		local out = decode(state)
		if out == nil then
			return nil
		end
		local ok, node = pcall(render, out)
		if not ok then
			if not state.warned then
				state.warned = true
				tern.log.warn(`pr-peek lens: render failed: {node}`)
			end
			return nil
		end
		return node
	end,
})

-- Hooks ----------------------------------------------------------------------

-- Make pr-peek emit JSON in every shell Tern spawns so the lens has data to
-- render. Shells opened before the plugin loaded won't have it; `--json` or
-- `export PR_PEEK_JSON=1` works there.
tern.on("spawn", function(spec: SpawnSpec): SpawnSpec?
	spec.env.PR_PEEK_JSON = "1"
	return spec
end)
