#!/usr/bin/env node
/**
 * Rewrite string className="..." to cn('...') using a bound CSS module map.
 * Conservative: only transforms literal className="..." and className={'...'}.
 */
import fs from 'node:fs';

/**
 * @param {string} file
 * @param {{ importPath: string, stylesName?: string, extraImports?: string[], mergeModules?: string[] }} opts
 */
export function wireFile(file, opts) {
  let src = fs.readFileSync(file, 'utf8');
  const stylesName = opts.stylesName ?? 'styles';
  const cnName = 'cn';

  if (src.includes(`from '${opts.importPath}'`) || src.includes(`from "${opts.importPath}"`)) {
    console.log('already wired?', file);
  }

  const imports = [];
  imports.push(`import ${stylesName} from '${opts.importPath}';`);
  for (const m of opts.mergeModules ?? []) {
    imports.push(`import ${m.name} from '${m.path}';`);
  }
  imports.push(`import { bind } from '${opts.bindPath ?? '../../shared/ui/cn'}';`);
  if (opts.mergeModules?.length) {
    const spread = [stylesName, ...opts.mergeModules.map((m) => m.name)]
      .map((n) => `...${n}`)
      .join(', ');
    imports.push(`const ${cnName} = bind({ ${spread} });`);
  } else {
    imports.push(`const ${cnName} = bind(${stylesName});`);
  }

  // Insert after last import
  const importBlock = imports.join('\n') + '\n';
  const lastImport = [...src.matchAll(/^import .+$/gm)].pop();
  if (!lastImport) throw new Error('no imports in ' + file);
  const insertAt = lastImport.index + lastImport[0].length;
  src = src.slice(0, insertAt) + '\n' + importBlock + src.slice(insertAt);

  // Transform className="foo bar" -> className={cn('foo', 'bar')}
  src = src.replace(/className="([^"{}\n]+)"/g, (_m, classes) => {
    const parts = classes.trim().split(/\s+/).filter(Boolean);
    if (parts.length === 0) return 'className=""';
    return `className={${cnName}(${parts.map((p) => JSON.stringify(p)).join(', ')})}`;
  });

  // className={`foo ${bar}`} leave alone — manual

  fs.writeFileSync(file, src);
  console.log('wired', file);
}

// CLI usage via inline list below when run as main
const root = new URL('../src/', import.meta.url).pathname;

const jobs = [
  {
    file: 'components/LayersPanel.tsx',
    importPath: '../features/layers/LayersPanel.module.css',
    bindPath: '../shared/ui/cn',
    mergeModules: [
      { name: 'retroSlider', path: '../shared/ui/RetroSlider.module.css' },
    ],
  },
  {
    file: 'components/common/DropdownMenu.tsx',
    importPath: './DropdownMenu.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'components/common/Slider.tsx',
    importPath: '../../shared/ui/Slider.module.css',
    bindPath: '../../shared/ui/cn',
    mergeModules: [{ name: 'retro', path: '../../shared/ui/RetroSlider.module.css' }],
  },
  {
    file: 'components/common/ResizeHandle.tsx',
    importPath: '../../shared/ui/ResizeHandle.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'components/common/Notification.tsx',
    importPath: '../../shared/ui/Notification.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'components/EmptyState.tsx',
    importPath: '../shared/ui/EmptyState.module.css',
    bindPath: '../shared/ui/cn',
  },
  {
    file: 'components/MenuBar.tsx',
    importPath: '../features/document/MenuBar.module.css',
    bindPath: '../shared/ui/cn',
  },
  {
    file: 'features/effects/EffectSettingsPanel.tsx',
    importPath: './EffectSettingsPanel.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'features/effects/editors/DitherSettings.tsx',
    importPath: '../EffectSettingsPanel.module.css',
    bindPath: '../../../shared/ui/cn',
    mergeModules: [
      { name: 'params', path: '../../../shared/ui/ParamControls.module.css' },
      { name: 'slider', path: '../../../shared/ui/Slider.module.css' },
      { name: 'buttons', path: '../../../shared/ui/FilterButtons.module.css' },
    ],
  },
  {
    file: 'features/effects/editors/GlitchSettings.tsx',
    importPath: '../EffectSettingsPanel.module.css',
    bindPath: '../../../shared/ui/cn',
    mergeModules: [
      { name: 'params', path: '../../../shared/ui/ParamControls.module.css' },
      { name: 'slider', path: '../../../shared/ui/Slider.module.css' },
      { name: 'input', path: '../../../shared/ui/ParamInput.module.css' },
    ],
  },
  {
    file: 'features/effects/editors/CurvesSettings.tsx',
    importPath: './CurvesSettings.module.css',
    bindPath: '../../../shared/ui/cn',
    mergeModules: [
      { name: 'panel', path: '../EffectSettingsPanel.module.css' },
      { name: 'params', path: '../../../shared/ui/ParamControls.module.css' },
      { name: 'slider', path: '../../../shared/ui/Slider.module.css' },
      { name: 'buttons', path: '../../../shared/ui/FilterButtons.module.css' },
    ],
  },
  {
    file: 'features/effects/editors/RGBSettings.tsx',
    importPath: './RGBSettings.module.css',
    bindPath: '../../../shared/ui/cn',
    mergeModules: [
      { name: 'panel', path: '../EffectSettingsPanel.module.css' },
      { name: 'slider', path: '../../../shared/ui/Slider.module.css' },
    ],
  },
  {
    file: 'components/PreviewWindow.tsx',
    importPath: '../features/preview/PreviewWindow.module.css',
    bindPath: '../shared/ui/cn',
    mergeModules: [{ name: 'preview', path: '../features/preview/Preview.module.css' }],
  },
  {
    file: 'app/AppLayout.tsx',
    importPath: './AppLayout.module.css',
    bindPath: '../shared/ui/cn',
    mergeModules: [
      { name: 'menu', path: '../features/document/MenuBar.module.css' },
      { name: 'resize', path: '../shared/ui/ResizeHandle.module.css' },
    ],
  },
  {
    file: 'shared/ui/WindowTitlebar.tsx',
    importPath: './WindowTitlebar.module.css',
    bindPath: './cn',
  },
  {
    file: 'features/panels/ColorLabDockStub.tsx',
    importPath: '../../shared/ui/FilterButtons.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'features/preview/PreviewSlot.tsx',
    importPath: './PreviewWindow.module.css',
    bindPath: '../../shared/ui/cn',
  },
  {
    file: 'components/EffectChooserDialog.tsx',
    importPath: '../features/effects/EffectChooserDialog.module.css',
    bindPath: '../shared/ui/cn',
    mergeModules: [{ name: 'titlebar', path: '../shared/ui/WindowTitlebar.module.css' }],
  },
  {
    file: 'features/color-lab/PalettePanel.tsx',
    importPath: './PalettePanel.module.css',
    bindPath: '../../shared/ui/cn',
    mergeModules: [
      { name: 'params', path: '../../shared/ui/ParamControls.module.css' },
      { name: 'buttons', path: '../../shared/ui/FilterButtons.module.css' },
      { name: 'slider', path: '../../shared/ui/Slider.module.css' },
      { name: 'input', path: '../../shared/ui/ParamInput.module.css' },
    ],
  },
  {
    file: 'components/ColorPicker.tsx',
    importPath: '../features/color-lab/ColorPicker.module.css',
    bindPath: '../shared/ui/cn',
  },
  {
    file: 'components/PanelWindow.tsx',
    importPath: '../features/panels/PanelChrome.module.css',
    bindPath: '../shared/ui/cn',
  },
];

for (const job of jobs) {
  wireFile(root + job.file, job);
}
