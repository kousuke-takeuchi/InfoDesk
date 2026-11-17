# CLAUDE.md

## デザイン

- デザインの正は OpenDesign のプロジェクト `infodesk` と、リポジトリの `design/opendesign/`(中身は同じ)。画面ごとの HTML が `screens/`、トークンが `tokens.css`、画面一覧が `index.html`。
- `design.pen` は移行元の pen.dev ファイルで、保存用。開かない、編集しない、pen.dev の MCP ツールも使わない。
- 画面を足す、直すときは OpenDesign(MCP の `open-design`、プロジェクト `infodesk`)か `design/opendesign/` を直接編集する。
- 片方を変えたら、もう片方へ同期してからコミットする。OpenDesign のフォルダは `~/Library/Application Support/Open Design/namespaces/release-stable/data/projects/infodesk/`(OpenDesign の `get_project` が返す `resolvedDir` で確かめる)。
  - OpenDesign からリポジトリへ: `rsync -a "<resolvedDir>/" design/opendesign/`
  - リポジトリから OpenDesign へ: `rsync -a design/opendesign/ "<resolvedDir>/"`
- 詳しくは `docs/internal/design-system.md`。
