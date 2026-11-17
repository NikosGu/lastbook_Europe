'use strict';
/* Runs in <head>, before the first paint: the classes on <html> that pick the look. No page content is touched here.
   plat-mac or plat-win: the desktop app by its address (as IN_WINDOW in app.js), a browser by its system. in-app: inside the desktop app.
   side-off: the sidebar was hidden. compact: Windows' folded pane of icons. inactive: the window is not in front. scrolled: the page is not at its top. */
(() => {
  const c = document.documentElement.classList;
  const inApp = location.protocol === 'lastbook:' || location.hostname === 'lastbook.invalid';
  const win = location.hostname === 'lastbook.invalid' || (!inApp && /^Win/.test(navigator.platform || ''));
  // The Linux app: a GTK window with its own title bar, so the page takes the browser's look (no traffic lights, no window material).
  const linuxApp = inApp && !win && /Linux/.test(navigator.platform || '');
  c.add(win ? 'plat-win' : 'plat-mac');
  if (inApp && !linuxApp) c.add('in-app');
  try { if (localStorage.getItem('sg.side-off') === '1') c.add('side-off') } catch { }
  const compact = () => c.toggle('compact', win && innerWidth >= 640 && (innerWidth < 900 || c.contains('side-off')));
  compact();
  addEventListener('resize', compact);
  const front = () => c.toggle('inactive', !document.hasFocus());
  addEventListener('blur', () => setTimeout(front, 0));   // later: focus moving into a frame of this page is still this window
  addEventListener('focus', front);
  front();   // also now: a window that opens behind another app starts inactive, until it is brought in front
  addEventListener('scroll', () => c.toggle('scrolled', scrollY > 2), { passive: true });
  window.LB_LAYOUT = compact;
})();
