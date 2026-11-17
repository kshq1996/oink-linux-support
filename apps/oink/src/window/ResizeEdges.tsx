import { inTauri } from '../api/transport';
import type { Platform } from './DesktopWindow';

const directions = ['North', 'NorthEast', 'East', 'SouthEast', 'South', 'SouthWest', 'West', 'NorthWest'] as const;

/** The visible shell sits inside the native window's transparent shadow gutter. */
export function ResizeEdges({ platform }: { platform: Platform }) {
  if (!inTauri) return null;
  return <div className="resize-edges" aria-hidden>
    {directions.map(direction => <div key={direction} data-direction={direction} onPointerDown={event => {
      // AppKit tracks Mac edge gestures before they reach the WebView because
      // Tao's startResizeDragging is unsupported there. Linux keeps the
      // desktop's own frame, which resizes the window itself.
      if (event.button !== 0 || platform !== 'win') return;
      event.preventDefault();
      void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => getCurrentWindow().startResizeDragging(direction));
    }}/>) }
  </div>;
}
