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
};

export function SkillMarkdownPreview({
  markdown,
  frontmatterLabel,
  noMarkdownLabel,
  noBodyLabel,
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
            pre: ({ children }) => <pre className="skill-code-block">{children}</pre>,
            code: ({ className, children }) => {
              const language = /language-([A-Za-z0-9_+-]+)/.exec(className ?? '')?.[1];
              return (
                <>
                  {language ? <span className="skill-code-language">{language}</span> : null}
                  <code className={className ?? 'inline-code'}>{children}</code>
                </>
              );
            },
          }}
        >
          {body}
        </ReactMarkdown>
      )}
    </div>
  );
}
