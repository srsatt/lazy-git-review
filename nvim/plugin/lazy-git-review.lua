if vim.g.loaded_lazy_git_review == 1 then
  return
end
vim.g.loaded_lazy_git_review = 1

require("lazy-git-review").setup()
