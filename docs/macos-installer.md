# macOS 安装盘

安装盘使用 720 × 508 点窗口（包含 28 点标题栏，内容为 720 × 480 点）、128 点原生图标、Luma 背景与拖拽箭头。用户将左侧 Luma 拖入右侧 Applications 安装，随后从应用程序打开。

## 构建

在 macOS 上执行（发布构建仍需要 updater 签名私钥）：

```bash
pnpm tauri build --bundles app
python3 -m venv /tmp/luma-dmg-tools
/tmp/luma-dmg-tools/bin/pip install -r scripts/requirements-dmg.txt
/tmp/luma-dmg-tools/bin/python scripts/build-macos-dmg.py \
  --app src-tauri/target/release/bundle/macos/Luma.app \
  --output /tmp/Luma-install.dmg
```

正式发布 workflow 使用 universal app，并自动以 tag 版本命名 DMG。DMG 只负责首次拖拽安装，应用内更新继续使用 Tauri 生成并签名的 `.app.tar.gz`。

背景与坐标来自 `src-tauri/tauri.conf.json` 的 `bundle.macOS.dmg`。脚本通过 dmgbuild 直接写入 `.DS_Store` 的背景引用、窗口和图标设置，无需操作 Finder。直接运行 Tauri 的 DMG bundler 不属于正式发布流程。

## 修改背景

`src-tauri/dmg/background.png` 为已提交的 2 倍分辨率背景，144 dpi 对应 720 × 480 点。常规构建不需要 Pillow 或字体依赖。修改设计时，在装有 Pillow 和苹方字体的 macOS 上运行：

```bash
python3 scripts/render-dmg-background.py
```

背景不写死版本，避免每次发布重新生成。

## 验收

挂载生成的 DMG，确认背景、箭头和两个大图标显示正常；关闭后重新打开，确认布局保留。检查 Applications 链接指向 `/Applications`，将 app 拖入应用程序并启动验证。预览可复用 debug app，但不能作为正式发行产物。
