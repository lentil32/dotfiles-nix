---@module "snacks"
---@param _ string
---@param type snacks.image.Type
---@return boolean
local function conceal_math(_, type)
  return type == "math"
end

---@return (number|string)[]
local function mermaid_args()
  local theme = vim.o.background == "light" and "neutral" or "dark"
  return { "-i", "{src}", "-o", "{file}", "-b", "transparent", "-t", theme, "-s", "{scale}" }
end

---@type table<string, snacks.image.args>
local magick = {
  default = { "{src}[0]", "-scale", "1920x1080>" },
  vector = { "-density", 192, "{src}[{page}]" },
  math = { "-density", 192, "{src}[{page}]", "-trim" },
  pdf = { "-density", 192, "{src}[{page}]", "-background", "white", "-alpha", "remove", "-trim" },
}

-- Local patch for snacks.nvim 2.31.0: upstream sends the loop index instead of
-- the actual placement id when deleting a single image placement.
local function patch_image_delete()
  local ok_image, image = pcall(require, "snacks.image.image")
  if not ok_image or type(image) ~= "table" or image.__myLuaConfDeletePatch then
    return
  end
  local ok_terminal, terminal = pcall(require, "snacks.image.terminal")
  if not ok_terminal or type(terminal) ~= "table" or type(terminal.request) ~= "function" then
    return
  end

  image.__myLuaConfDeletePatch = true

  ---@param pid? number
  function image:del(pid)
    local placements = self.placements or {}
    for _, placement_id in ipairs(pid and { pid } or vim.tbl_keys(placements)) do
      if placements[placement_id] then
        terminal.request({ a = "d", d = "i", i = self.id, p = placement_id })
        placements[placement_id] = nil
      end
    end

    if not next(placements) then
      terminal.request({ a = "d", d = "i", i = self.id })
    end
  end
end

-- Local patch: when a placement has no visible windows, upstream hides it without
-- deleting the terminal image, which can leave stale graphics behind.
local function patch_hidden_placement_update()
  local ok_placement, placement = pcall(require, "snacks.image.placement")
  if not ok_placement or type(placement) ~= "table" or placement.__myLuaConfHiddenPatch then
    return
  end
  if type(placement.update) ~= "function" then
    return
  end

  placement.__myLuaConfHiddenPatch = true
  local update = placement.update

  function placement:update()
    if not self:ready() then
      return
    end
    if not self:valid() then
      self:del()
      return
    end
    if #self:state().wins == 0 then
      self._state = nil
      self:del()
      return
    end
    return update(self)
  end
end

local function install_patches()
  patch_image_delete()
  patch_hidden_placement_update()
end

---@type snacks.image.Config
local image_opts = {
  enabled = true,
  config = function()
    -- `config` runs while snacks.image is still loading, so install after the
    -- module finishes to avoid recursive require errors.
    vim.schedule(install_patches)
  end,
  install_patches = install_patches,
  formats = {
    "png",
    "jpg",
    "jpeg",
    "gif",
    "bmp",
    "webp",
    "tiff",
    "heic",
    "avif",
    "mp4",
    "mov",
    "avi",
    "mkv",
    "webm",
    "pdf",
    "icns",
  },
  force = false,
  doc = {
    enabled = true,
    inline = true,
    float = true,
    max_width = 80,
    max_height = 40,
    conceal = conceal_math,
  },
  img_dirs = { "img", "images", "assets", "static", "public", "media", "attachments" },
  wo = {
    wrap = false,
    number = false,
    relativenumber = false,
    cursorcolumn = false,
    signcolumn = "no",
    foldcolumn = "0",
    list = false,
    spell = false,
    statuscolumn = "",
  },
  cache = vim.fn.stdpath("cache") .. "/snacks/image",
  debug = {
    request = false,
    convert = false,
    placement = false,
  },
  env = {},
  icons = {
    math = "󰪚 ",
    chart = "󰄧 ",
    image = " ",
  },
  convert = ({
    notify = false,
    mermaid = mermaid_args,
    magick = magick,
  }),
  math = {
    enabled = true,
    typst = {
      tpl = [[
        #set page(width: auto, height: auto, margin: (x: 2pt, y: 2pt))
        #show math.equation.where(block: false): set text(top-edge: "bounds", bottom-edge: "bounds")
        #set text(size: 12pt, fill: rgb("${color}"))
        ${header}
        ${content}]],
    },
    latex = {
      font_size = "Large",
      packages = { "amsmath", "amssymb", "amsfonts", "amscd", "mathtools" },
      tpl = [[
        \documentclass[preview,border=0pt,varwidth,12pt]{standalone}
        \usepackage{${packages}}
        \begin{document}
        ${header}
        { \${font_size} \selectfont
          \color[HTML]{${color}}
        ${content}}
        \end{document}]],
    },
  },
}

return image_opts
