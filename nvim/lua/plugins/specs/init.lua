local specs = {}

local function add(list)
  vim.list_extend(specs, list)
end

add(require("plugins.specs.ui"))
add(require("plugins.specs.motion"))
add(require("plugins.specs.navigation"))
add(require("plugins.specs.git"))
add(require("plugins.specs.org"))
add(require("plugins.specs.syntax"))
add(require("plugins.specs.lsp"))
add(require("plugins.specs.typescript"))
add(require("plugins.specs.format"))

return specs
