---@meta

---@class rs_snacks_preview.ImageRef
---@field src? string

---@class rs_snacks_preview.DocFindArgs
---@field buf integer
---@field token integer
---@field win integer
---@field imgs? rs_snacks_preview.ImageRef[]

---@class rs_snacks_preview.AttachDocPreviewArgs
---@field buf integer
---@field win integer
---@field path string

---@class rs_snacks_preview
---@field on_doc_find fun(args: rs_snacks_preview.DocFindArgs)
---@field attach_doc_preview fun(args: rs_snacks_preview.AttachDocPreviewArgs)
---@field close_doc_preview fun(buf: integer)
---@field reset_state fun()

---@type rs_snacks_preview
local M = {}

---@param args rs_snacks_preview.DocFindArgs
function M.on_doc_find(args) end

---@param args rs_snacks_preview.AttachDocPreviewArgs
function M.attach_doc_preview(args) end

---@param buf integer
function M.close_doc_preview(buf) end

function M.reset_state() end

return M
