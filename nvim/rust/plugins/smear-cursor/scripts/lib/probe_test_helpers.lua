local M = {}

function M.prepend_runtimepath(path)
  if path == nil or path == "" then
    return
  end
  vim.opt.runtimepath:prepend(path)
end

function M.assert_probe_result(actual, expected, context)
  if actual.color ~= expected.color then
    error(
      string.format(
        "%s: expected color %s, got %s",
        context,
        tostring(expected.color),
        tostring(actual.color)
      )
    )
  end

  if actual.used_extmark_fallback ~= expected.used_extmark_fallback then
    error(
      string.format(
        "%s: expected used_extmark_fallback=%s, got %s",
        context,
        tostring(expected.used_extmark_fallback),
        tostring(actual.used_extmark_fallback)
      )
    )
  end
end

function M.reset_probe_module()
  package.loaded["nvimrs_smear_cursor.probes"] = nil
  return require("nvimrs_smear_cursor.probes")
end

function M.with_mocked_syntax(syntax, callback)
  local original_syn_id = vim.fn.synID
  local original_syn_idtrans = vim.fn.synIDtrans
  local original_syn_idattr = vim.fn.synIDattr

  vim.fn.synID = function()
    return syntax.id or 0
  end

  vim.fn.synIDtrans = function(id)
    if syntax.trans_id ~= nil then
      return syntax.trans_id
    end
    return id
  end

  vim.fn.synIDattr = function(_, attr)
    if attr == "fg#" then
      return syntax.fg or ""
    end
    if attr == "name" then
      return syntax.name or ""
    end
    return ""
  end

  local ok, result = pcall(callback)
  vim.fn.synID = original_syn_id
  vim.fn.synIDtrans = original_syn_idtrans
  vim.fn.synIDattr = original_syn_idattr
  if not ok then
    error(result)
  end
  return result
end

return M
