function escapeHtml(s: string): string {
  return String(s)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
}

function escapeAttr(s: string): string {
  return String(s)
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
}

function safeHref(raw: string): string {
  const href = String(raw || '').trim();
  const lower = href.toLowerCase();
  if (!href) return '#';
  if (
    lower.startsWith('http://') ||
    lower.startsWith('https://') ||
    lower.startsWith('mailto:')
  )
    return href;
  if (
    href.startsWith('#') ||
    href.startsWith('/') ||
    href.startsWith('./') ||
    href.startsWith('../')
  )
    return href;
  return '#';
}

function renderEmphasis(escaped: string): string {
  return String(escaped)
    .replaceAll(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replaceAll(/\*([^*]+)\*/g, '<em>$1</em>');
}

function renderInlineNoCode(raw: string): string {
  const s = String(raw ?? '');
  let out = '';
  let idx = 0;
  const linkRe = /\[([^\]]+)\]\(([^)]+)\)/g;
  for (;;) {
    const m = linkRe.exec(s);
    if (!m) break;
    out += renderEmphasis(escapeHtml(s.slice(idx, m.index)));
    const label = renderEmphasis(escapeHtml(m[1] ?? ''));
    const href = escapeAttr(safeHref(m[2] ?? ''));
    out += `<a href="${href}" target="_blank" rel="noreferrer noopener">${label}</a>`;
    idx = m.index + m[0].length;
  }
  out += renderEmphasis(escapeHtml(s.slice(idx)));
  return out;
}

function renderInline(raw: string): string {
  const text = String(raw ?? '');
  let out = '';
  let i = 0;
  while (i < text.length) {
    const tick = text.indexOf('`', i);
    if (tick === -1) {
      out += renderInlineNoCode(text.slice(i));
      break;
    }
    const end = text.indexOf('`', tick + 1);
    if (end === -1) {
      out += renderInlineNoCode(text.slice(i));
      break;
    }
    out += renderInlineNoCode(text.slice(i, tick));
    out += `<code>${escapeHtml(text.slice(tick + 1, end))}</code>`;
    i = end + 1;
  }
  return out;
}

export function renderMarkdown(md: string): string {
  const lines = String(md ?? '')
    .replaceAll('\r\n', '\n')
    .split('\n');
  const out: string[] = [];
  let paragraph: string[] = [];
  let listType = '';
  let inCodeFence = false;
  let codeLang = '';
  let code: string[] = [];
  let inQuote = false;
  let quote: string[] = [];

  function flushParagraph() {
    if (!paragraph.length) return;
    const text = paragraph.join('\n').trim().replaceAll(/\n+/g, ' ');
    out.push(`<p>${renderInline(text)}</p>`);
    paragraph = [];
  }

  function flushList() {
    if (!listType) return;
    out.push(listType === 'ol' ? '</ol>' : '</ul>');
    listType = '';
  }

  function flushQuote() {
    if (!inQuote) return;
    const text = quote.join('\n').trim().replaceAll(/\n+/g, ' ');
    out.push(`<blockquote>${text ? `<p>${renderInline(text)}</p>` : ''}</blockquote>`);
    inQuote = false;
    quote = [];
  }

  function closeBlocks() {
    flushParagraph();
    flushList();
    flushQuote();
  }

  for (const line of lines) {
    if (inCodeFence) {
      if (line.startsWith('```')) {
        const klass = codeLang ? ` class="language-${escapeAttr(codeLang)}"` : '';
        out.push(`<pre><code${klass}>${escapeHtml(code.join('\n'))}</code></pre>`);
        inCodeFence = false;
        codeLang = '';
        code = [];
      } else {
        code.push(line);
      }
      continue;
    }

    if (line.startsWith('```')) {
      closeBlocks();
      inCodeFence = true;
      codeLang = line.slice(3).trim();
      code = [];
      continue;
    }

    if (/^\s*$/.test(line)) {
      flushParagraph();
      flushList();
      flushQuote();
      continue;
    }

    const quoteMatch = line.match(/^\s*>\s?(.*)$/);
    if (quoteMatch) {
      flushParagraph();
      flushList();
      inQuote = true;
      quote.push(quoteMatch[1] ?? '');
      continue;
    }
    flushQuote();

    if (/^\s*((\*\s*){3,}|(-\s*){3,}|(_\s*){3,})$/.test(line)) {
      closeBlocks();
      out.push('<hr>');
      continue;
    }

    const headingMatch = line.match(/^(#{1,6})\s+(.*)$/);
    if (headingMatch) {
      closeBlocks();
      const lvl = headingMatch[1]?.length ?? 1;
      out.push(`<h${lvl}>${renderInline(headingMatch[2]?.trim() ?? '')}</h${lvl}>`);
      continue;
    }

    const ulMatch = line.match(/^\s*[-*+]\s+(.*)$/);
    if (ulMatch) {
      flushParagraph();
      flushQuote();
      if (listType && listType !== 'ul') flushList();
      if (!listType) {
        listType = 'ul';
        out.push('<ul>');
      }
      out.push(`<li>${renderInline(ulMatch[1]?.trim() ?? '')}</li>`);
      continue;
    }

    const olMatch = line.match(/^\s*\d+\.\s+(.*)$/);
    if (olMatch) {
      flushParagraph();
      flushQuote();
      if (listType && listType !== 'ol') flushList();
      if (!listType) {
        listType = 'ol';
        out.push('<ol>');
      }
      out.push(`<li>${renderInline(olMatch[1]?.trim() ?? '')}</li>`);
      continue;
    }

    paragraph.push(line);
  }

  if (inCodeFence) {
    const klass = codeLang ? ` class="language-${escapeAttr(codeLang)}"` : '';
    out.push(`<pre><code${klass}>${escapeHtml(code.join('\n'))}</code></pre>`);
  }
  closeBlocks();
  return out.join('\n');
}
