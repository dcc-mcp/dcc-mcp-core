import { Children, isValidElement, type ReactNode, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

function splitSkillMarkdown(markdown: string): { frontmatter: string | null; body: string } {
  const normalized = markdown.replace(/\r\n/g, '\n');
  const lines = normalized.split('\n');
  if (lines[0]?.trim() === '---') {
    const end = lines.findIndex((line, index) => index > 0 && line.trim() === '---');
    if (end > 0) {
      return {
        frontmatter: lines.slice(1, end).join('\n').trim(),
        body: lines.slice(end + 1).join('\n').trim(),
      };
    }
  }
  return { frontmatter: null, body: normalized.trim() };
}

type SkillMarkdownPreviewProps = {
  markdown?: string | null;
  frontmatterLabel: string;
  noMarkdownLabel: string;
  noBodyLabel: string;
  copyLabel: string;
  copiedLabel: string;
};

function nodeText(node: ReactNode): string {
  if (node == null || typeof node === 'boolean') return '';
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join('');
  if (isValidElement<{ children?: ReactNode }>(node)) return nodeText(node.props.children);
  return '';
}

function SkillCodeBlock({
  children,
  copyLabel,
  copiedLabel,
}: {
  children: ReactNode;
  copyLabel: string;
  copiedLabel: string;
}) {
  const [copied, setCopied] = useState(false);
  const child = Children.toArray(children).find((item) => isValidElement(item));
  const codeProps = isValidElement<{ className?: string; children?: ReactNode }>(child)
    ? child.props
    : {};
  const language = /language-([A-Za-z0-9_+-]+)/.exec(codeProps.className ?? '')?.[1];
  const code = nodeText(codeProps.children ?? children).replace(/\n$/, '');

  const copyCode = async () => {
    try {
      await navigator.clipboard?.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="skill-code-card">
      <div className="skill-code-toolbar">
        <span className="skill-code-language">{language ?? 'text'}</span>
        <button className="skill-code-copy" type="button" onClick={() => void copyCode()}>
          {copied ? copiedLabel : copyLabel}
        </button>
      </div>
      <pre className="skill-code-block">
        <code className={codeProps.className}>{codeProps.children ?? children}</code>
      </pre>
    </div>
  );
}

export function SkillMarkdownPreview({
  markdown,
  frontmatterLabel,
  noMarkdownLabel,
  noBodyLabel,
  copyLabel,
  copiedLabel,
}: SkillMarkdownPreviewProps) {
  if (!markdown) {
    return <p className="empty">{noMarkdownLabel}</p>;
  }
  const { frontmatter, body } = splitSkillMarkdown(markdown);
  return (
    <div className="skill-markdown-preview">
      {frontmatter ? (
        <details className="skill-frontmatter">
          <summary>{frontmatterLabel}</summary>
          <pre>{frontmatter}</pre>
        </details>
      ) : null}
      {!body ? <p className="empty">{noBodyLabel}</p> : (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            h1: ({ children }) => <h3>{children}</h3>,
            h2: ({ children }) => <h4>{children}</h4>,
            h3: ({ children }) => <h5>{children}</h5>,
            a: ({ href, children }) => (
              <a href={href} target="_blank" rel="noopener noreferrer">{children}</a>
            ),
            table: ({ children }) => <div className="skill-table-wrap"><table>{children}</table></div>,
            pre: ({ children }) => (
              <SkillCodeBlock copyLabel={copyLabel} copiedLabel={copiedLabel}>
                {children}
              </SkillCodeBlock>
            ),
            code: ({ className, children }) => {
              const language = /language-([A-Za-z0-9_+-]+)/.exec(className ?? '')?.[1];
              return <code className={language ? className : `${className ?? ''} inline-code`.trim()}>{children}</code>;
            },
          }}
        >
          {body}
        </ReactMarkdown>
      )}
    </div>
  );
}
