---@param _ string
---@param type snacks.image.Type
---@return boolean
local function conceal_math(_, type)
  return type == "math"
end

---@return snacks.image.args
local function mermaid_args()
  local theme = vim.o.background == "light" and "neutral" or "dark"
  return { "-i", "{src}", "-o", "{file}", "-b", "transparent", "-t", theme, "-s", "{scale}" }
end

local convert = {
  magick = {
    default = { "{src}[0]", "-scale", "1920x1080>" },
    vector = { "-density", 192, "{src}[{page}]" },
    math = { "-density", 192, "{src}[{page}]", "-trim" },
    pdf = { "-density", 192, "{src}[{page}]", "-background", "white", "-alpha", "remove", "-trim" },
  },
}

if not vim.g.neovide then
  convert.mermaid = mermaid_args
end

---@type snacks.image.Config
local image_opts = {
  enabled = true,
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
  convert = convert,
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
