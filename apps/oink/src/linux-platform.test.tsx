// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { agent } from './api/store';
import { setLanguage } from './i18n';

vi.mock('./api/store', async original => ({ ...await original<typeof import('./api/store')>(), start: vi.fn() }));
vi.mock('./api/transport', async original => ({
  ...await original<typeof import('./api/transport')>(),
  transport: {
    call: async ({ method }: { method: string }) => method === 'group' ? { members: [] } : [],
    shell: async () => null,
    onEvent: async () => () => {},
  },
}));
beforeAll(() => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  Object.defineProperty(window, 'matchMedia', { value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) });
});
afterEach(() => { cleanup(); agent.reset(); localStorage.clear(); });

function boot(platform: 'macos' | 'windows' | 'linux') {
  setLanguage('en');
  agent.adopt('boot', { platform, version: '0.1.0', build: null, backend: 'agent' });
  return render(<App/>).container;
}

describe('the Linux shell', () => {
  it('reports the Linux platform and leaves the window buttons to the desktop frame', () => {
    const container = boot('linux');
    const win = container.querySelector('.win')!;
    expect(win.getAttribute('data-platform')).toBe('linux');
    expect(container.querySelector('.oink-app')?.getAttribute('data-platform')).toBe('linux');
    expect(win.querySelector('.mac-lights')).toBeNull();
    expect(win.querySelector('.win-caps')).toBeNull();
  });
  it('uses the sidebar navigation instead of the Windows taskbar layout', () => {
    const container = boot('linux');
    const win = container.querySelector('.win')!;
    expect(win.querySelector('.win__nav-back')).toBeNull();
    expect(win.querySelector('.nav-view > .addp__head-fold')).not.toBeNull();
    expect(win.querySelector('.nav-view > .addp__group')).not.toBeNull();
    expect(win.querySelector('.nav-stack > .addp__group')).toBeNull();
  });
  it('names the Control key in the undo and redo hints', () => {
    boot('linux');
    expect(screen.getByRole('button', { name: 'Undo' }).getAttribute('aria-keyshortcuts')).toBe('Control+Z');
    expect(screen.getByRole('button', { name: 'Redo' }).getAttribute('aria-keyshortcuts')).toBe('Control+Y');
  });
});

describe('the macOS and Windows shells', () => {
  it('keeps the macOS traffic lights and the Windows window buttons', () => {
    const mac = boot('macos');
    expect(mac.querySelector('.win')?.getAttribute('data-platform')).toBe('mac');
    expect(mac.querySelector('.mac-lights')).not.toBeNull();
    cleanup();
    const win = boot('windows');
    expect(win.querySelector('.win')?.getAttribute('data-platform')).toBe('win');
    expect(win.querySelector('.win-caps')).not.toBeNull();
    expect(win.querySelector('.nav-stack > .addp__group')).not.toBeNull();
  });
});
