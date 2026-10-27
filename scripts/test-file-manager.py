#!/usr/bin/env python3
"""UI regression against a running Vite server using isolated Tauri IPC mocks.

python3 -m pip install playwright
playwright install webkit
pnpm dev --host 127.0.0.1
python3 scripts/test-file-manager.py --browser all

Real file mutations and persistence are covered by cargo test, not these mocks.
"""
import argparse
import json
import os
from pathlib import Path
import tempfile
from playwright.sync_api import sync_playwright, expect

parser=argparse.ArgumentParser()
parser.add_argument('--browser',choices=['chromium','webkit','all'],default='all')
parser.add_argument('--browser-path')
parser.add_argument('--workspace-only',action='store_true')
parser.add_argument('--layout-only',action='store_true')
parser.add_argument('--base-url',default='http://127.0.0.1:1030')
parser.add_argument('--output',default=str(Path(tempfile.gettempdir())/'luma-file-manager-review'))
args=parser.parse_args()
output=Path(args.output);output.mkdir(parents=True,exist_ok=True)
fixture=Path(__file__).with_name('file-manager-fixture.html').read_text()


def load(page,query=''):
    page.goto(args.base_url+'/__file_manager_test__?'+query)
    expect(page.get_by_role('button',name='整理',exact=True)).to_be_visible()


def open_wizard(page,english=False):
    page.get_by_role('button',name='Organize' if english else '整理',exact=True).click()
    dialog=page.get_by_role('dialog')
    expect(dialog).to_be_visible()
    expect(dialog.locator('.org-rule-card')).to_have_count(5)
    # A visible outer dialog alone missed the original WebKit regression.
    assert dialog.locator('.org-wizard-body').bounding_box()['height'] >= 230
    expect(dialog.locator('.org-rule-card').first).to_be_in_viewport()
    assert dialog.evaluate('(el)=>el.scrollWidth<=el.clientWidth+1')
    return dialog


def preview(page,english=False):
    dialog=open_wizard(page,english)
    page.get_by_role('button',name='Next' if english else '下一步',exact=True).click()
    expect(page.get_by_role('checkbox',name='Include files inside subfolders' if english else '包含子文件夹中的文件')).not_to_be_checked()
    page.get_by_role('button',name='Generate preview' if english else '生成预览',exact=True).click()
    expect(page.get_by_role('button',name='Confirm moving 3 files' if english else '确认移动 3 个文件',exact=True)).to_be_enabled()
    return dialog


def run_workspace(page,engine):
    # The folder is the parent context of both persistent workspace views.
    for lang in ['zh-CN','en-US']:
        english=lang=='en-US'
        page.goto(args.base_url+'/__file_manager_test__?workspace=1&lang='+lang)
        analysis=page.get_by_role('tab',name='Space analysis' if english else '空间分析',exact=True)
        manager=page.get_by_role('tab',name='File manager' if english else '文件管理',exact=True)
        expect(analysis).to_be_enabled()
        expect(page.get_by_role('heading',name='Space overview' if english else '空间概览',exact=True)).to_be_visible()
        folder=page.locator('.workspace-controls .path-button')
        expect(folder).to_contain_text('/Users/demo/Desktop')
        assert folder.bounding_box()['y'] < analysis.bounding_box()['y']
        position=folder.bounding_box()
        page.screenshot(path=str(output/f'{engine}-workspace-analysis-{lang}.png'))
        page.locator('.analysis-tab-panel').evaluate('element=>element.scrollTop=250')
        scroll=page.locator('.analysis-tab-panel').evaluate('element=>element.scrollTop')
        expect(folder).to_be_in_viewport()
        manager.click()
        expect(page.locator('.fm-view')).to_be_visible()
        assert folder.bounding_box()==position
        expect(page.get_by_role('button',name='Rescan' if english else '重新扫描',exact=True)).to_be_visible()
        page.screenshot(path=str(output/f'{engine}-workspace-files-{lang}.png'))
        # Switching views retains a navigated directory and analysis scroll position.
        page.locator('.fm-main .fm-file-row--dir').click()
        expect(page.get_by_text('project.txt',exact=True)).to_be_visible()
        analysis.click()
        assert page.locator('.analysis-tab-panel').evaluate('element=>element.scrollTop')==scroll
        manager.click()
        expect(page.get_by_text('project.txt',exact=True)).to_be_visible()
        # Changing the root from file management never leaves old results on screen.
        page.evaluate('window.test.picker="/Users/demo/Downloads"')
        folder.click()
        expect(folder).to_contain_text('/Users/demo/Downloads')
        expect(analysis).to_be_disabled()
        expect(manager).to_be_disabled()
        expect(page.locator('.fm-view')).not_to_be_visible()
        expect(page.get_by_role('heading',name='Folder selected. Scan to get started.' if english else '目录已选好，扫描后开始使用',exact=True)).to_be_visible()
        page.screenshot(path=str(output/f'{engine}-workspace-changed-{lang}.png'))
        page.get_by_role('button',name='Scan' if english else '扫描',exact=True).click()
        expect(page.locator('.fm-view')).to_be_visible(timeout=10000)
        expect(manager).to_have_attribute('aria-selected','true')
        assert page.evaluate("window.calls.filter(c=>c.cmd==='list_directory_files').at(-1).args.dirPath")=="/Users/demo/Downloads"
        # Folder controls and view switches fit the supported minimum window.
        page.set_viewport_size({'width':760,'height':560})
        expect(folder).to_be_in_viewport()
        expect(manager).to_be_in_viewport()
        expect(page.locator('.fm-toolbar')).to_be_in_viewport()
        assert page.locator('.workspace-controls').evaluate('element=>element.scrollWidth<=element.clientWidth')
        page.screenshot(path=str(output/f'{engine}-workspace-minimum-{lang}.png'))
        # Keyboard navigation follows the two tabs.
        manager.focus();manager.press('ArrowLeft')
        expect(analysis).to_have_attribute('aria-selected','true')
        expect(analysis).to_be_focused()
        page.set_viewport_size({'width':1120,'height':760})


def run_storage(page,engine):
    for lang in ['zh-CN','en-US']:
        english=lang=='en-US'
        page.set_viewport_size({'width':760,'height':560})
        page.goto(args.base_url+'/__file_manager_test__?workspace=1&lang='+lang)
        title='Luma local storage' if english else 'Luma 本地存储'
        trigger=page.get_by_role('button',name=title,exact=True)
        expect(trigger).to_be_enabled()
        page.evaluate('window.test.storageDelay=800')
        trigger.click()
        dialog=page.get_by_role('dialog',name=title)
        expect(dialog).to_have_attribute('aria-busy','true')
        expect(dialog.get_by_role('button',name='Close' if english else '关闭',exact=True)).to_be_disabled()
        expect(dialog).to_contain_text('40.0 MB')
        assert dialog.evaluate('el=>el.scrollWidth<=el.clientWidth+1')
        compact=dialog.get_by_role('button',name='Reclaim unused space' if english else '回收空闲空间',exact=True)
        compact.click()
        expect(compact).to_be_disabled()
        expect(compact).to_have_attribute('aria-busy','true')
        expect(dialog.get_by_role('status')).to_contain_text('Unused space reclaimed.' if english else '空闲空间已回收。')
        assert page.evaluate("window.calls.filter(c=>c.cmd==='compact_index_storage').length")==1
        dialog.get_by_role('button',name='Clear scan index' if english else '清理扫描索引',exact=True).click()
        expect(dialog.get_by_text('Clear all scan indexes?' if english else '清理所有扫描索引？',exact=True)).to_be_visible()
        page.screenshot(path=str(output/f'{engine}-storage-confirm-{lang}.png'))
        dialog.get_by_role('button',name='Cancel' if english else '取消',exact=True).click()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='clear_scan_index').length")==0
        dialog.get_by_role('button',name='Clear scan index' if english else '清理扫描索引',exact=True).click()
        page.evaluate('window.test.failClear=true')
        dialog.get_by_role('button',name='Confirm clearing index' if english else '确认清理索引',exact=True).click()
        expect(dialog.get_by_role('alert')).to_have_text('Storage unavailable')
        expect(dialog.get_by_role('button',name='Confirm clearing index' if english else '确认清理索引',exact=True)).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='get_latest_scan').length")>=2
        assert page.evaluate("window.calls.filter(c=>c.cmd==='clear_scan_index').length")==1
        page.evaluate('window.test.failClear=false')
        clear=dialog.get_by_role('button',name='Confirm clearing index' if english else '确认清理索引',exact=True)
        clear.click()
        expect(clear).to_be_disabled()
        expect(clear.locator('.button-spinner')).to_be_visible()
        expect(dialog).to_contain_text('Original files and undo records are kept.' if english else '原文件与撤销记录已保留。')
        assert page.evaluate("window.calls.filter(c=>c.cmd==='clear_scan_index').length")==2
        expect(dialog.locator('.index-storage-stats div').nth(2).locator('dd')).to_have_text('2')
        page.screenshot(path=str(output/f'{engine}-storage-cleared-{lang}.png'))
        dialog.get_by_role('button',name='Close' if english else '关闭',exact=True).click()
        expect(page.get_by_role('tab',name='File manager' if english else '文件管理',exact=True)).to_be_disabled()
        expect(page.locator('.workspace-controls .path-button')).to_contain_text('/Users/demo/Desktop')
        # Recover from a failed storage query and leave scanning available.
        page.evaluate('window.test.failStorage=true')
        trigger.click()
        expect(dialog.get_by_role('alert')).to_have_text('Storage unavailable')
        page.evaluate('window.test.failStorage=false')
        dialog.get_by_role('button',name='Retry' if english else '重试',exact=True).click()
        expect(dialog.locator('.index-storage-total')).to_be_visible()
        page.keyboard.press('Escape')
        expect(dialog).not_to_be_visible()
        assert page.locator('.app-header').evaluate('el=>el.scrollWidth<=el.clientWidth+1')
    page.set_viewport_size({'width':1120,'height':760})


def review_layout(page,engine):
    # Capture the actual app shell, including selection and compact preview states.
    for lang in ['zh-CN','en-US']:
        english=lang=='en-US'
        for width,height in [(1120,760),(760,560)]:
            page.set_viewport_size({'width':width,'height':height})
            page.goto(args.base_url+'/__file_manager_test__?workspace=1&lang='+lang)
            page.get_by_role('tab',name='File manager' if english else '文件管理',exact=True).click()
            expect(page.locator('.fm-file-row--header')).to_be_visible()
            prefix=f'{engine}-layout-{lang}-{width}'
            page.screenshot(path=str(output/f'{prefix}-list.png'))
            page.get_by_text('最新笔记.txt',exact=True).click()
            expect(page.locator('.fm-preview-text')).to_have_text('LATEST CONTENT')
            expect(page.locator('.fm-preview-actions .button').last).to_be_in_viewport()
            page.screenshot(path=str(output/f'{prefix}-preview.png'))
            page.get_by_role('button',name='Close preview' if english else '关闭预览',exact=True).click()
            for name in ['预算.xlsx','最新笔记.txt']:
                page.get_by_role('checkbox',name=('Select ' if english else '选择 ')+name,exact=True).check()
            expect(page.get_by_role('button',name='Copy' if english else '复制',exact=True)).to_be_in_viewport()
            assert page.locator('.fm-view').evaluate('el=>el.scrollWidth<=el.clientWidth')
            page.screenshot(path=str(output/f'{prefix}-selection.png'))
            page.get_by_role('button',name='Clear' if english else '清除',exact=True).click()
            page.get_by_role('button',name='Grid view' if english else '网格视图',exact=True).click()
            expect(page.get_by_role('grid')).to_be_visible()
            page.screenshot(path=str(output/f'{prefix}-grid.png'))
            open_wizard(page,english)
            page.screenshot(path=str(output/f'{prefix}-organize.png'))
            page.keyboard.press('Escape')
    page.set_viewport_size({'width':1120,'height':760})


def run_suite(browser,engine):
    page=browser.new_page(viewport={'width':1120,'height':760})
    errors=[]
    diagnostics=[]
    page.on('pageerror',lambda error:errors.append(str(error)))
    page.on('console',lambda message:diagnostics.append(message.text) if message.type in ['error','warning'] else None)
    page.route('**/__file_manager_test__*',lambda route:route.fulfill(status=200,content_type='text/html',body=fixture))
    try:
        if args.layout_only:
            review_layout(page,engine)
            assert not errors,errors
            print(json.dumps({'engine':engine,'status':'passed','suite':'layout','screenshots':str(output)},ensure_ascii=False),flush=True)
            return
        if args.workspace_only:
            run_workspace(page,engine)
            run_storage(page,engine)
            assert not errors,errors
            print(json.dumps({'engine':engine,'status':'passed','suite':'workspace','screenshots':str(output)},ensure_ascii=False),flush=True)
            return
        load(page)
        # Leaving a rename draft must not swallow the first click on another action.
        page.get_by_role('button',name='重命名 客户A-合同.pdf',exact=True).click()
        page.get_by_role('textbox',name='重命名文件',exact=True).fill('draft.pdf')
        page.get_by_role('button',name='整理',exact=True).click()
        expect(page.get_by_role('dialog')).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='rename_file').length")==0
        page.keyboard.press('Escape')
        page.get_by_role('button',name='重命名 客户A-合同.pdf',exact=True).click()
        page.get_by_role('textbox',name='重命名文件',exact=True).fill('saved.pdf')
        page.get_by_role('button',name='保存重命名',exact=True).click()
        expect(page.get_by_text('saved.pdf',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='rename_file').length")==1
        page.get_by_role('button',name='重命名 saved.pdf',exact=True).click()
        page.get_by_role('textbox',name='重命名文件',exact=True).fill('discarded.pdf')
        page.get_by_role('button',name='取消重命名',exact=True).click()
        expect(page.get_by_text('saved.pdf',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='rename_file').length")==1
        load(page)
        # A slow native picker is locked from the first click, including cancellation.
        page.evaluate('Object.assign(window.test,{pickerDelay:800,picker:null})')
        page.get_by_role('checkbox',name='选择 最新笔记.txt',exact=True).check()
        copy=page.get_by_role('button',name='复制',exact=True)
        copy.evaluate('button=>{button.click();button.click();button.click()}')
        expect(copy).to_have_attribute('aria-busy','true')
        expect(copy).to_be_disabled()
        expect(copy.locator('.button-spinner')).to_be_visible()
        expect(page.get_by_text('等待选择目标目录…',exact=True)).to_be_visible()
        page.screenshot(path=str(output/f'{engine}-copy-loading.png'))
        expect(copy).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='plugin:dialog|open').length")==1
        assert page.evaluate("window.calls.filter(c=>c.cmd==='copy_files').length")==0
        page.evaluate('Object.assign(window.test,{pickerDelay:0,picker:"/Users/demo/Desktop",planDelay:800})')
        dialog=open_wizard(page)
        page.get_by_role('button',name='下一步',exact=True).click()
        source=dialog.locator('.org-dir-btn').first
        page.evaluate('window.test.pickerDelay=800')
        source.evaluate('button=>{button.click();button.click();button.click()}')
        expect(source).to_have_attribute('aria-busy','true')
        expect(source).to_be_disabled()
        generate=page.get_by_role('button',name='生成预览',exact=True)
        expect(generate).to_be_disabled()
        expect(generate).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='plugin:dialog|open').length")==2
        generate.evaluate('button=>{button.click();button.click();button.click()}')
        generating=page.get_by_role('button',name='生成预览中…',exact=True)
        expect(generating).to_be_disabled()
        expect(generating).to_have_attribute('aria-busy','true')
        expect(generating.locator('.button-spinner')).to_be_visible()
        page.screenshot(path=str(output/f'{engine}-preview-loading.png'))
        expect(page.get_by_role('button',name='确认移动 3 个文件',exact=True)).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='plan_organize').length")==1
        page.keyboard.press('Escape')
        # Slow directory refreshes preserve the controls and block repeat clicks.
        page.evaluate('window.test.listDelay=800')
        hidden=page.get_by_role('button',name='显示隐藏文件',exact=True)
        hidden.click()
        expect(page.locator('.fm-toolbar-fieldset')).to_have_attribute('disabled','')
        expect(page.get_by_text('正在读取目录…',exact=True)).to_be_visible()
        expect(page.get_by_text('.hidden.txt',exact=True)).to_be_visible()
        # Failed open/reveal calls show an error and release the button lock.
        page.evaluate('Object.assign(window.test,{openDelay:800,failOpen:true})')
        page.get_by_text('客户A-合同.pdf',exact=True).click()
        open_button=page.locator('.fm-toolbar').get_by_role('button',name='打开',exact=True)
        open_button.evaluate('button=>{button.click();button.click();button.click()}')
        expect(open_button).to_be_disabled()
        expect(open_button).to_have_attribute('aria-busy','true')
        expect(page.get_by_text('Open unavailable',exact=True)).to_be_visible()
        expect(open_button).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='open_path').length")==1
        load(page)
        # File columns share positions with headers and directory rows.
        header=page.locator('.fm-file-row--header')
        row=page.locator('.fm-file-rows .fm-file-row').nth(1)
        for cell in [2,3,4]:
            left=header.locator(':scope > span').nth(cell).bounding_box()['x']
            actual=row.locator(':scope > span').nth(cell).bounding_box()['x']
            assert abs(left-actual)<2,(engine,cell,left,actual)
        page.screenshot(path=str(output/f'{engine}-files.png'))
        dialog=open_wizard(page)
        page.screenshot(path=str(output/f'{engine}-rules.png'))
        page.get_by_role('button',name='下一步',exact=True).click()
        page.screenshot(path=str(output/f'{engine}-folders.png'))
        page.get_by_role('button',name='添加规则',exact=True).click()
        page.get_by_role('textbox',name='关键词，例如：客户A',exact=True).fill('客户A')
        page.get_by_role('textbox',name='目标子目录',exact=True).fill('客户A资料')
        page.get_by_role('button',name='生成预览',exact=True).click()
        expect(page.get_by_role('button',name='确认移动 3 个文件',exact=True)).to_be_enabled()
        page.screenshot(path=str(output/f'{engine}-preview.png'))
        page.get_by_role('checkbox',name='移动 客户A-合同.pdf',exact=True).uncheck()
        page.get_by_role('textbox',name='分类 文档 的文件夹名称',exact=True).fill('工作资料')
        expect(page.get_by_role('button',name='确认移动 2 个文件',exact=True)).to_be_disabled()
        page.get_by_role('button',name='更新预览',exact=True).click()
        expect(page.get_by_role('checkbox',name='移动 客户A-合同.pdf',exact=True)).not_to_be_checked()
        page.get_by_role('combobox',name='同名文件',exact=True).select_option('rename')
        page.get_by_role('button',name='更新预览',exact=True).click()
        page.get_by_role('textbox',name='预算.xlsx 的目标分类',exact=True).fill('财务')
        page.get_by_role('button',name='更新预览',exact=True).click()
        expect(page.get_by_role('textbox',name='分类 财务 的文件夹名称',exact=True)).to_be_visible()
        page.get_by_role('button',name='全部保持原位',exact=True).click()
        expect(page.get_by_role('button',name='确认移动 0 个文件',exact=True)).to_be_disabled()
        page.get_by_role('button',name='选择全部可移动文件',exact=True).click()
        page.get_by_role('checkbox',name='移动 客户A-合同.pdf',exact=True).uncheck()
        page.get_by_role('button',name='确认移动 3 个文件',exact=True).click()
        expect(page.get_by_text('成功移动 3 个文件',exact=True)).to_be_visible()
        expect(page.locator('.org-wizard-title')).to_have_text('整理完成')
        page.screenshot(path=str(output/f'{engine}-result.png'))
        payload=page.evaluate("window.calls.find(c=>c.cmd==='execute_organize_plan').args")
        assert len(payload['moves'])==3 and not any('客户A' in m['from'] for m in payload['moves'])
        assert any('/财务/' in m['to'] for m in payload['moves'])
        assert any('(2).pdf' in m['to'] for m in payload['moves'])
        page.get_by_role('button',name='完成',exact=True).click()
        undo=page.get_by_role('button',name='撤销',exact=True)
        expect(undo).to_be_enabled();undo.click();expect(undo).not_to_be_visible()
        # Navigation clears old checks and keeps the root sidebar stable.
        page.get_by_role('checkbox',name='选择 客户A-合同.pdf',exact=True).check()
        page.locator('.fm-main').get_by_role('button',name='Project 10 项 1000 B',exact=True).click()
        expect(page.get_by_text('project.txt',exact=True)).to_be_visible()
        expect(page.locator('.fm-toolbar-selection-count')).not_to_be_visible()
        expect(page.locator('.fm-sidebar').get_by_role('button',name='Project 1000 B',exact=True)).to_be_visible()
        page.get_by_role('button',name='Desktop',exact=True).click()
        # Editing keys must not open files or trigger trash; Enter commits once.
        page.get_by_role('button',name='重命名 客户A-合同.pdf',exact=True).click()
        name=page.get_by_role('textbox',name='重命名文件',exact=True)
        name.fill('new-name.pdf');name.press('Backspace')
        expect(page.get_by_role('dialog')).not_to_be_visible()
        name.fill('renamed.pdf');name.press('Enter')
        expect(page.get_by_text('renamed.pdf',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='rename_file').length")==1
        assert page.evaluate("window.calls.filter(c=>c.cmd==='open_path'||c.cmd==='trash_files').length")==0
        # A slow text preview cannot overwrite the newly selected file.
        page.get_by_text('慢速笔记.txt',exact=True).click()
        page.get_by_text('最新笔记.txt',exact=True).click()
        expect(page.locator('.fm-preview-text')).to_have_text('LATEST CONTENT')
        page.wait_for_timeout(700)
        expect(page.locator('.fm-preview-text')).to_have_text('LATEST CONTENT')
        page.get_by_text('Screenshot 2026-10-26.png',exact=True).click()
        image=page.locator('.fm-preview-image')
        expect(image).to_be_visible()
        expect(image).to_have_js_property('naturalWidth',1)
        page.get_by_text('renamed.pdf',exact=True).click()
        pdf=page.get_by_role('img',name='PDF 预览',exact=True)
        expect(pdf).to_be_visible(timeout=20000)
        # Assert that PDF text produced dark pixels, not just an empty canvas.
        assert pdf.evaluate('''canvas => {
          const {data}=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height);
          let dark=0;
          for(let i=0;i<data.length;i+=4) if(data[i]<100 && data[i+1]<100 && data[i+2]<100 && data[i+3]>0) dark++;
          return dark>100;
        }''')
        page.screenshot(path=str(output/f'{engine}-pdf.png'))
        page.evaluate('window.test.invalidPdf=true')
        page.get_by_text('其他资料.pdf',exact=True).click()
        expect(page.locator('.fm-preview-pdf [role="status"]')).to_contain_text('无法读取文件内容。')
        page.evaluate('window.test.failBinary=true')
        page.get_by_text('Screenshot 2026-10-26.png',exact=True).click()
        expect(page.locator('.fm-preview [role="status"]')).to_have_text('Preview too large')
        page.get_by_text('预算.xlsx',exact=True).click()
        expect(page.locator('.fm-preview-text')).not_to_be_visible()
        expect(page.get_by_text('此格式请使用默认应用打开查看。',exact=True)).to_be_visible()
        # System dialog errors become visible feedback, not unhandled promises.
        page.evaluate('window.test.failPicker=true')
        page.get_by_role('checkbox',name='选择 预算.xlsx',exact=True).check()
        page.get_by_role('button',name='移动',exact=True).click()
        expect(page.get_by_text('Picker unavailable',exact=True)).to_be_visible()
        # Successful copy/move use the selected snapshot; trash requires confirmation.
        load(page)
        page.evaluate('window.test.slowOps=true')
        page.get_by_role('checkbox',name='选择 最新笔记.txt',exact=True).check()
        page.get_by_role('button',name='复制',exact=True).click()
        expect(page.get_by_role('button',name='复制',exact=True)).to_be_disabled()
        expect(page.locator('.fm-body')).to_have_attribute('inert','')
        expect(page.get_by_text('已复制 1 个文件',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='copy_files').length")==1
        copied=page.evaluate("window.calls.find(c=>c.cmd==='copy_files').args")
        assert copied['paths']==['/Users/demo/Desktop/最新笔记.txt']
        page.get_by_role('checkbox',name='选择 最新笔记.txt',exact=True).check()
        page.get_by_role('button',name='移动',exact=True).click()
        move_dialog=page.get_by_role('dialog')
        expect(move_dialog).to_contain_text('/Users/demo/Desktop/Destination')
        expect(move_dialog).to_contain_text('/Users/demo/Desktop')
        expect(move_dialog).to_contain_text('即将移动 1 个文件')
        page.screenshot(path=str(output/f'{engine}-move-confirm.png'))
        assert page.evaluate("window.calls.filter(c=>c.cmd==='move_files').length")==0
        expect(move_dialog.get_by_role('button',name='取消',exact=True)).to_be_focused()
        page.keyboard.press('Escape')
        assert page.evaluate("window.calls.filter(c=>c.cmd==='move_files').length")==0
        page.get_by_role('button',name='移动',exact=True).click()
        page.get_by_role('dialog').get_by_role('button',name='确认移动',exact=True).click()
        expect(page.get_by_role('button',name='移动',exact=True)).to_be_disabled()
        expect(page.get_by_text('已移动 1 个文件',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='move_files').length")==1
        page.get_by_role('checkbox',name='选择 最新笔记.txt',exact=True).check()
        page.get_by_role('button',name='废纸篓',exact=True).click()
        confirm=page.get_by_role('dialog')
        expect(confirm.get_by_role('button',name='取消',exact=True)).to_be_focused()
        page.keyboard.press('Escape')
        assert page.evaluate("window.calls.filter(c=>c.cmd==='trash_files').length")==0
        page.get_by_role('button',name='废纸篓',exact=True).click()
        # A native modal keeps Tab inside its two choices.
        page.keyboard.press('Tab')
        expect(page.get_by_role('dialog').get_by_role('button',name='移至废纸篓',exact=True)).to_be_focused()
        page.get_by_role('dialog').get_by_role('button',name='移至废纸篓',exact=True).click()
        expect(page.get_by_text('已将 1 个文件移至废纸篓',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='trash_files').length")==1
        # Hidden filtering clears selection; sorting and grid switching remain usable.
        load(page)
        page.get_by_role('checkbox',name='选择 最新笔记.txt',exact=True).check()
        page.get_by_role('button',name='显示隐藏文件',exact=True).click()
        expect(page.get_by_text('.hidden.txt',exact=True)).to_be_visible()
        expect(page.locator('.fm-toolbar-selection-count')).not_to_be_visible()
        page.get_by_role('combobox',name='排序方式',exact=True).select_option('nameDesc')
        expect(page.get_by_role('combobox',name='排序方式',exact=True)).to_have_value('nameDesc')
        page.get_by_role('button',name='网格视图',exact=True).click()
        expect(page.get_by_role('grid')).to_be_visible()
        page.get_by_role('button',name='列表视图',exact=True).click()
        expect(page.locator('.fm-file-rows')).to_be_visible()
        # Failed regeneration keeps execution disabled.
        load(page);preview(page)
        page.get_by_role('textbox',name='分类 文档 的文件夹名称',exact=True).fill('../escape')
        page.get_by_role('button',name='更新预览',exact=True).click()
        expect(page.get_by_role('alert')).to_be_visible()
        expect(page.get_by_role('button',name='确认移动 3 个文件',exact=True)).to_be_disabled()
        page.keyboard.press('Escape');expect(page.get_by_role('dialog')).not_to_be_visible()
        # Empty plans cannot execute; a failed execution retains its result dialog.
        load(page);page.evaluate('window.test.emptyPlan=true');preview_empty=open_wizard(page)
        page.get_by_role('button',name='下一步',exact=True).click()
        page.get_by_role('button',name='生成预览',exact=True).click()
        expect(page.get_by_role('button',name='确认移动 0 个文件',exact=True)).to_be_disabled()
        page.keyboard.press('Escape')
        load(page);preview(page);page.evaluate('window.test.failExecute=true')
        page.get_by_role('button',name='确认移动 3 个文件',exact=True).click()
        expect(page.get_by_text('Execution unavailable',exact=True)).to_be_visible()
        expect(page.get_by_role('button',name='完成',exact=True)).to_be_enabled()
        page.get_by_role('button',name='完成',exact=True).click()
        expect(page.get_by_role('dialog')).not_to_be_visible()
        # Every step retains a scrollable body and visible footer at small sizes.
        for lang in ['zh-CN','en-US']:
            page.goto(args.base_url+'/__file_manager_test__?lang='+lang)
            english=lang=='en-US'
            expect(page.get_by_role('button',name='Organize' if english else '整理',exact=True)).to_be_visible()
            for size in [(760,560),(390,720)]:
                page.set_viewport_size({'width':size[0],'height':size[1]})
                dialog=open_wizard(page,english)
                expect(dialog.locator('.org-wizard-footer .button').last).to_be_in_viewport()
                page.screenshot(path=str(output/f'{engine}-rules-{lang}-{size[0]}.png'))
                dialog.locator('.org-rule-card').last.scroll_into_view_if_needed()
                expect(dialog.locator('.org-rule-card').last).to_be_in_viewport()
                page.get_by_role('button',name='Next' if english else '下一步',exact=True).click()
                expect(dialog.locator('.org-wizard-footer .button').last).to_be_in_viewport()
                page.screenshot(path=str(output/f'{engine}-folders-{lang}-{size[0]}.png'))
                page.get_by_role('button',name='Generate preview' if english else '生成预览',exact=True).click()
                execute=page.get_by_role('button',name='Confirm moving 3 files' if english else '确认移动 3 个文件',exact=True)
                expect(execute).to_be_in_viewport()
                assert dialog.locator('.org-wizard-body').evaluate('element=>element.scrollTop')==0
                page.screenshot(path=str(output/f'{engine}-preview-{lang}-{size[0]}.png'))
                execute.click()
                done=page.get_by_role('button',name='Done' if english else '完成',exact=True)
                expect(done).to_be_in_viewport()
                page.screenshot(path=str(output/f'{engine}-result-{lang}-{size[0]}.png'))
                done.click()
        page.set_viewport_size({'width':1120,'height':760})
        # More than two pages detects resetting page=0 after every fetch.
        load(page,'pages=1')
        page.get_by_role('button',name='下一页',exact=True).click()
        expect(page.locator('.fm-pagination-info')).to_have_text('第 2 / 3 页')
        expect(page.get_by_text('file-100.txt',exact=True)).to_be_visible()
        page.get_by_role('button',name='下一页',exact=True).click()
        expect(page.locator('.fm-pagination-info')).to_have_text('第 3 / 3 页')
        expect(page.get_by_text('file-204.txt',exact=True)).to_be_visible()
        expect(page.get_by_role('button',name='下一页',exact=True)).to_be_disabled()
        page.get_by_role('checkbox',name='全选当前页',exact=True).check()
        page.get_by_role('button',name='废纸篓',exact=True).click()
        page.get_by_role('dialog').get_by_role('button',name='移至废纸篓',exact=True).click()
        expect(page.locator('.fm-pagination-info')).to_have_text('第 2 / 2 页')
        expect(page.get_by_text('file-100.txt',exact=True)).to_be_visible()
        page.get_by_role('button',name='上一页',exact=True).click()
        expect(page.get_by_text('file-000.txt',exact=True)).to_be_visible()
        # Scan controls also lock while the native picker/start/cancel IPC is pending.
        page.goto(args.base_url+'/__file_manager_test__?app=1')
        picker=page.locator('.path-button')
        expect(picker).to_be_enabled()
        page.evaluate('window.test.pickerDelay=800')
        picker.evaluate('button=>{button.click();button.click();button.click()}')
        expect(picker).to_be_disabled()
        expect(picker).to_have_attribute('aria-busy','true')
        expect(picker.locator('.button-spinner')).to_be_visible()
        expect(picker).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='plugin:dialog|open').length")==1
        scan=page.get_by_role('button',name='扫描',exact=True)
        scan.evaluate('button=>{button.click();button.click();button.click()}')
        starting=page.get_by_role('button',name='正在启动扫描…',exact=True)
        expect(starting).to_be_disabled()
        expect(starting).to_have_attribute('aria-busy','true')
        cancel=page.get_by_role('button',name='取消',exact=True)
        expect(cancel).to_be_enabled()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='start_scan').length")==1
        cancel.evaluate('button=>{button.click();button.click();button.click()}')
        expect(page.get_by_role('button',name='正在取消…',exact=True)).to_be_disabled()
        expect(page.get_by_text('扫描已取消',exact=True)).to_be_visible()
        assert page.evaluate("window.calls.filter(c=>c.cmd==='cancel_scan').length")==1
        run_workspace(page,engine)
        run_storage(page,engine)
        assert not errors,errors
        print(json.dumps({'engine':engine,'status':'passed','screenshots':str(output)},ensure_ascii=False),flush=True)
    except Exception:
        page.screenshot(path=str(output/f'{engine}-failure.png'))
        (output/f'{engine}-failure.txt').write_text(page.locator('body').inner_text()+'\n\nErrors: '+str(errors)+'\nConsole: '+str(diagnostics))
        raise
    finally:
        page.close()

if args.browser_path:
    os.environ['PLAYWRIGHT_BROWSERS_PATH']=args.browser_path

with sync_playwright() as playwright:
    engines=['chromium','webkit'] if args.browser=='all' else [args.browser]
    for engine in engines:
        browser=playwright.chromium.launch(channel='chrome') if engine=='chromium' else playwright.webkit.launch()
        try:run_suite(browser,engine)
        finally:browser.close()
