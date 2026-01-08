---@meta

---@class Project.DisableOn
---@field ft string[]
---@field bt string[]

---@class Project.Config.Options: table
---@field disable_on Project.DisableOn

---@class Project.Config: table
---@field options Project.Config.Options

---@class Project.API: table
---@field valid_bt fun(bufnr?: integer): boolean
---@field get_project_root fun(bufnr?: integer): string|nil
---@field set_pwd fun(dir?: string, method?: string): boolean|nil
