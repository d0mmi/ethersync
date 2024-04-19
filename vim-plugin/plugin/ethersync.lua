local utils = require("utils")
local sync = require("vim.lsp.sync")

-- Used to note that changes to the buffer should be ignored, and not be sent out as deltas.
local ignore_edits = false
-- Same, but for use in async code. TODO: Occasionally clean it up.
local ignored_changedticks = {}

-- JSON-RPC connection.
local client

-- Toggle to simulate the editor going offline.
local online = false

-- Queues filled during simulated "offline" mode, and consumed when we go online again.
local opQueueForDaemon = {}
local opQueueForEditor = {}

-- Number of operations the daemon has made.
local daemonRevision = 0
-- Number of operations we have made.
local editorRevision = 0

-- Used to remember the previous content of the buffer, so that we can
-- calculate the difference between the previous and the current content.
local prev_lines

local function debug(tbl)
    if true then
        client.notify("debug", tbl)
    end
end

local function applyDelta(delta)
    local text_edits = {}
    for _, replacement in ipairs(delta) do
        local text_edit = {
            range = {
                start = replacement.range.anchor,
                ["end"] = replacement.range.head,
            },
            newText = replacement.replacement,
        }
        table.insert(text_edits, text_edit)
    end

    ignore_edits = true
    local changedtick_before = vim.api.nvim_buf_get_changedtick(0)
    vim.lsp.util.apply_text_edits(text_edits, 0, "utf-32")
    local changedtick_after = vim.api.nvim_buf_get_changedtick(0)
    ignore_edits = false

    debug({ changedtick_before = changedtick_before, changedtick_after = changedtick_after })

    daemonRevision = daemonRevision + 1
end

-- Take an operation from the daemon and apply it to the editor.
local function processOperationForEditor(method, parameters)
    if method == "edit" then
        local _uri = parameters.uri --[[@diagnostic disable-line]]
        local delta = parameters.delta.delta
        local theEditorRevision = parameters.delta.revision

        if theEditorRevision == editorRevision then
            applyDelta(delta)
            EnsureNewlineAtEndOfFile(true)
        else
            -- Operation is not up-to-date to our content, skip it!
            -- The daemon will send a transformed one later.
            print(
                "Skipping operation, my editor revision is "
                    .. editorRevision
                    .. " but operation is for revision "
                    .. theEditorRevision
            )
        end
    else
        print("Unknown method: " .. method)
    end
end

-- Reset the state on editor side and re-open the current buffer
--
-- (this is to be called on buffer change, once we have the ability to detect that)
local function resetState()
    daemonRevision = 0
    editorRevision = 0
    opQueueForDaemon = {}
    opQueueForEditor = {}
end

-- Connect to the daemon.
local function connect(socket_path)
    resetState()

    local params = { "client" }
    if socket_path then
        table.insert(params, "--socket-path=" .. socket_path)
    end
    client = vim.lsp.rpc.start("ethersync", params, {
        notification = function(method, notification_params)
            if online then
                processOperationForEditor(method, notification_params)
            else
                table.insert(opQueueForEditor, { method, notification_params })
            end
        end,
    })
    online = true

    local filename = "file://" .. vim.fs.basename(vim.api.nvim_buf_get_name(0))
    client.notify("open", { uri = filename })
end

local function connect2()
    if client then
        client.terminate()
    end
    connect("/tmp/etherbonk")
end

-- Simulate disconnecting from the daemon.
local function goOffline()
    online = false
end

-- Simulate connecting to the daemon again.
-- Apply both queues, then reset them.
local function goOnline()
    for _, op in ipairs(opQueueForDaemon) do
        local method = op[1]
        local params = op[2]
        client.notify(method, params)
    end

    for _, op in ipairs(opQueueForEditor) do
        local method = op[1]
        local params = op[2]
        processOperationForEditor(method, params)
    end

    opQueueForDaemon = {}
    opQueueForEditor = {}
    online = true
end

-- Initialization function.
function Ethersync()
    if vim.fn.isdirectory(vim.fn.expand("%:p:h") .. "/.ethersync") ~= 1 then
        return
    end

    print("Ethersync activated!")

    connect()

    prev_lines = vim.api.nvim_buf_get_lines(0, 0, -1, true)

    -- If there are no lines, set 'eol' to true. We didn't find a way to tell if the file contains '\n' or ''.
    if #prev_lines == 0 then
        vim.bo.eol = false
    end

    vim.api.nvim_buf_attach(0, false, {
        on_lines = function(
            _the_literal_string_lines --[[@diagnostic disable-line]],
            _buffer_handle --[[@diagnostic disable-line]],
            _changedtick, --[[@diagnostic disable-line]]
            first_line,
            last_line,
            new_last_line
        )
            -- Line counts that we get called with are zero-based.
            -- last_line and new_last_line are exclusive

            debug({ first_line = first_line, last_line = last_line, new_last_line = new_last_line })
            -- TODO: optimize with a cache
            local curr_lines = vim.api.nvim_buf_get_lines(0, 0, -1, true)

            -- Are we currently ignoring edits?
            local current_changedtick = vim.api.nvim_buf_get_changedtick(0)
            local current_changedtick_is_ignored = ignored_changedticks[current_changedtick]
            if ignore_edits or current_changedtick_is_ignored then
                prev_lines = curr_lines
                return
            end

            editorRevision = editorRevision + 1

            debug({ curr_lines = curr_lines, prev_lines = prev_lines })
            local diff = sync.compute_diff(prev_lines, curr_lines, first_line, last_line, new_last_line, "utf-32", "\n")
            -- line/character indices in diff are zero-based.
            debug({ diff = diff })

            -- Sometimes, Vim deletes full lines by deleting the last line, plus an imaginary newline at the end. For example, to delete the second line, Vim would delete from (line: 1, column: 0) to (line: 2, column 0).
            -- But, in the case of deleting the last line, what we expect in the rest of Ethersync is to delete the newline *before* the line.
            -- So let's change the deleted range to (line: 0, column: [last character of the first line]) to (line: 1, column: [last character of the second line]).

            local rev_delta = {
                delta = {
                    {
                        range = {
                            anchor = diff.range.start,
                            head = diff.range["end"],
                        },
                        replacement = diff.text,
                    },
                },
                revision = daemonRevision,
            }

            local uri = "file://" .. vim.api.nvim_buf_get_name(0)
            local params = { uri = uri, delta = rev_delta }

            if online then
                client.notify("edit", params)
            else
                table.insert(opQueueForDaemon, { "edit", params })
            end

            EnsureNewlineAtEndOfFile(true)

            prev_lines = curr_lines
        end,
    })

    local send_this_out = not vim.api.nvim_get_option_value("eol", { buf = 0 })
    EnsureNewlineAtEndOfFile(send_this_out)

    -- Always make the implicit newline at the end of the file visible.
    --if vim.api.nvim_get_option_value("fixeol", { buf = 0 }) then
    --    if not vim.api.nvim_get_option_value("eol", { buf = 0 }) then
    --        utils.appendNewline()
    --        vim.api.nvim_set_option_value("eol", true, { buf = 0 })
    --    end
    --    vim.api.nvim_set_option_value("fixeol", false, { buf = 0 })
    --end
end

function EnsureNewlineAtEndOfFile(send_this_out)
    vim.schedule(function()
        if not send_this_out then
            local next_changedtick = vim.api.nvim_buf_get_changedtick(0) + 1
            ignored_changedticks[next_changedtick] = true
        end
        local last_line_array = vim.api.nvim_buf_get_lines(0, -2, -1, true)
        local last_line = last_line_array[1]
        print(vim.inspect(last_line))
        if #last_line > 0 then
            local row = vim.api.nvim_buf_line_count(0) - 1
            local col = #last_line
            local cursor_position = vim.api.nvim_win_get_cursor(0)

            vim.api.nvim_buf_set_text(0, row, col, row, col, { "", "" })
            vim.api.nvim_win_set_cursor(0, cursor_position)
        end
    end)
end

function EthersyncClose()
    if vim.fn.isdirectory(vim.fn.expand("%:p:h") .. "/.ethersync") ~= 1 then
        return
    end

    local filename = "file://" .. vim.fs.basename(vim.api.nvim_buf_get_name(0))
    client.notify("close", { uri = filename })
end

-- When new buffer is loaded, run Ethersync automatically.
vim.api.nvim_exec(
    [[
augroup Ethersync
    autocmd!
    autocmd BufEnter * lua Ethersync()
    autocmd BufUnload * lua EthersyncClose()
augroup END
]],
    false
)

vim.api.nvim_create_user_command("Ethersync", Ethersync, {})

vim.api.nvim_create_user_command("EthersyncRunTests", utils.testAllUnits, {})
vim.api.nvim_create_user_command("EthersyncGoOffline", goOffline, {})
vim.api.nvim_create_user_command("EthersyncGoOnline", goOnline, {})
vim.api.nvim_create_user_command("EthersyncReload", resetState, {})
vim.api.nvim_create_user_command("Etherbonk", connect2, {})
