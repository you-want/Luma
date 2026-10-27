#!/usr/bin/env python3
"""Build the macOS drag-to-install image without Finder automation."""
import argparse
import json
import sys
from pathlib import Path

import dmgbuild


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('DMG creation requires macOS.')
    app = args.app.resolve()
    if not app.is_dir() or app.suffix != '.app':
        parser.error('--app must point to a built .app bundle.')
    root = Path(__file__).resolve().parent.parent
    config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
    layout = config['bundle']['macOS']['dmg']
    point = lambda value: (value['x'], value['y'])
    # Unlike Tauri's Finder AppleScript, dmgbuild writes the background alias
    # and icon sizes directly to .DS_Store, including on headless CI runners.
    settings = {
        'format': 'UDZO',
        'files': [str(app)],
        'symlinks': {'Applications': '/Applications'},
        'background': str(root / 'src-tauri' / layout['background']),
        'icon': str(root / 'src-tauri/icons/icon.icns'),
        'window_rect': (
            point(layout['windowPosition']),
            # Finder's window bounds include its 28 pt title bar; the artwork
            # and native icon coordinates refer to the content below it.
            (layout['windowSize']['width'], layout['windowSize']['height']),
        ),
        'icon_locations': {
            app.name: point(layout['appPosition']),
            'Applications': point(layout['applicationFolderPosition']),
        },
        'icon_size': 128,
        'text_size': 13,
        'hide_extensions': [app.name],
        'default_view': 'icon-view',
        'include_icon_view_settings': True,
        'include_list_view_settings': False,
        'show_toolbar': False,
        'show_status_bar': False,
        'show_sidebar': False,
        'show_pathbar': False,
    }
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    dmgbuild.build_dmg(str(output), config['productName'], settings=settings)
    print(output)


if __name__ == '__main__':
    main()
