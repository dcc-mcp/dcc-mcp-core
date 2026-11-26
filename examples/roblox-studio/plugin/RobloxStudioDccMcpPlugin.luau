-- Roblox Studio plugin side of the dcc-mcp-core bridge example.
-- This script requires the Studio-only WebStreamClient WebSocket API.

local HttpService = game:GetService("HttpService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local BRIDGE_URL = "ws://127.0.0.1:9001"
local PLUGIN_VERSION = "0.1.0"

local toolbar = plugin:CreateToolbar("DCC-MCP")
local toggleButton = toolbar:CreateButton(
	"DccMcpRobloxStudio",
	"Connect Roblox Studio to dcc-mcp-core",
	""
)

local client = nil
local heartbeatSequence = 0
local INSTANCE_ID = HttpService:GenerateGUID(false)

local function safeStudioVersion()
	local ok, value = pcall(function()
		return version()
	end)
	return ok and value or "unknown"
end

local function send(message)
	if client and client.ConnectionState == Enum.WebStreamClientState.Open then
		client:Send(HttpService:JSONEncode(message))
	end
end

local function instanceAtPath(path)
	local current = game
	for part in string.gmatch(path or "game", "[^%.]+") do
		if part ~= "game" then
			current = current:FindFirstChild(part)
			if not current then
				return nil
			end
		end
	end
	return current
end

local function instanceSummary(instance)
	local children = {}
	for _, child in ipairs(instance:GetChildren()) do
		table.insert(children, {
			name = child.Name,
			class_name = child.ClassName,
			path = child:GetFullName(),
		})
	end
	return {
		name = instance.Name,
		class_name = instance.ClassName,
		path = instance:GetFullName(),
		children = children,
		attributes = instance:GetAttributes(),
	}
end

local function searchGameTree(params)
	local query = string.lower(params.query or "")
	local root = instanceAtPath(params.root or "game") or game
	local maxResults = math.clamp(params.max_results or 50, 1, 500)
	local results = {}
	for _, instance in ipairs(root:GetDescendants()) do
		local haystack = string.lower(instance.Name .. " " .. instance.ClassName .. " " .. instance:GetFullName())
		if query == "" or string.find(haystack, query, 1, true) then
			table.insert(results, {
				name = instance.Name,
				class_name = instance.ClassName,
				path = instance:GetFullName(),
			})
			if #results >= maxResults then
				break
			end
		end
	end
	return {results = results, count = #results}

end

local function readScript(params)
	local scriptInstance = instanceAtPath(params.path)
	if not scriptInstance or not scriptInstance:IsA("LuaSourceContainer") then
		error("path does not resolve to a script")
	end
	return {
		path = scriptInstance:GetFullName(),
		class_name = scriptInstance.ClassName,
		source = scriptInstance.Source,
	}
end

local function applyMultiEdit(params)
	local edits = params.edits or {}
	if #edits < 1 or #edits > 32 then
		error("edits must contain between 1 and 32 entries")
	end
	local targets = {}
	for _, edit in ipairs(edits) do
		local scriptInstance = instanceAtPath(edit.path)
		if not scriptInstance or not scriptInstance:IsA("LuaSourceContainer") then
			error("edit path does not resolve to a script: " .. tostring(edit.path))
		end
		if type(edit.new_source) ~= "string" then
			error("new_source must be a string: " .. tostring(edit.path))
		end
		table.insert(targets, {instance = scriptInstance, source = edit.new_source})
	end

	local recording = ChangeHistoryService:TryBeginRecording(
		"dcc_mcp_multi_edit",
		"DCC-MCP Roblox Studio multi-edit"
	)
	if not recording then
		error("could not start a Studio change recording")
	end

	local changed = {}
	local ok, failure = pcall(function()
		for _, target in ipairs(targets) do
			target.instance.Source = target.source
			table.insert(changed, target.instance:GetFullName())
		end
	end)
	if ok then
		ChangeHistoryService:FinishRecording(recording, Enum.FinishRecordingOperation.Commit)
	else
		ChangeHistoryService:FinishRecording(recording, Enum.FinishRecordingOperation.Cancel)
		error(failure)
	end
	return {changed = changed, count = #changed}
end

local handlers = {
	["roblox.get_studio_state"] = function()
		return {
			place_id = game.PlaceId,
			game_id = game.GameId,
			name = game.Name,
			is_edit = game:GetService("RunService"):IsEdit(),
			is_running = game:GetService("RunService"):IsRunning(),
			data_model_type = "Edit",
		}
	end,
	["roblox.search_game_tree"] = searchGameTree,
	["roblox.inspect_instance"] = function(params)
		local instance = instanceAtPath(params.path)
		if not instance then
			error("instance not found: " .. tostring(params.path))
		end
		return instanceSummary(instance)
	end,
	["roblox.script_read"] = readScript,
	["roblox.multi_edit"] = applyMultiEdit,
}

local function handleMessage(raw)
	local message = HttpService:JSONDecode(raw)
	if message.type ~= "request" then
		return
	end
	local handler = handlers[message.method]
	if not handler then
		send({
			type = "response",
			jsonrpc = "2.0",
			id = message.id,
			error = {code = -32601, message = "method not found: " .. tostring(message.method)},
		})
		return
	end
	local ok, result = pcall(handler, message.params or {})
	if ok then
		send({type = "response", jsonrpc = "2.0", id = message.id, result = result})
	else
		send({
			type = "response",
			jsonrpc = "2.0",
			id = message.id,
			error = {code = -32000, message = tostring(result)},
		})
	end
end

local function connect()
	if client then
		client:Close()
		client = nil
	end
	client = HttpService:CreateWebStreamClient(Enum.WebStreamClientType.WebSocket, {
		Url = BRIDGE_URL,
		Method = "GET",
	})
	client.Opened:Connect(function()
		send({
			type = "hello",
			protocol_version = "1",
			client = "roblox-studio-plugin",
			version = PLUGIN_VERSION,
			dcc_version = safeStudioVersion(),
            instance_id = INSTANCE_ID,
			peer_id = "studio-plugin",
			host_name = "Roblox Studio",
			host_version = safeStudioVersion(),
			document = tostring(game.PlaceId),
			project = tostring(game.GameId),
			data_model = "DataModel",
			capabilities = {
				get_studio_state = true,
				search_game_tree = true,
				inspect_instance = true,
				script_read = true,
				multi_edit = true,
			},
		})
		send({type = "ready", ready = true, state = "ready"})
	end)
	client.MessageReceived:Connect(handleMessage)
	client.Error:Connect(function(_, message)
		warn("DCC-MCP Roblox bridge error: " .. tostring(message))
	end)
	client.Closed:Connect(function()
		client = nil
	end)

	task.spawn(function()
		while client do
			task.wait(5)
			if not client then
				break
			end
			heartbeatSequence += 1
			send({type = "heartbeat", sequence = heartbeatSequence, ready = true})
		end
	end)
end

toggleButton.Click:Connect(function()
	if client then
		client:Close()
		client = nil
	else
		connect()
	end
end)

connect()
