import { Fragment, type ReactNode } from "react";

/**
 * Tiny, safe Markdown renderer for stored session messages.
 *
 * Sessions can hold arbitrary text from many agents, so this renderer never
 * injects HTML: links are restricted to http/https and every other character
 * goes through React's normal escaping. It covers the shapes that actually
 * show up in transcripts — headings, lists, fenced/inline code, emphasis and
 * links — rather than trying to be a full CommonMark implementation.
 */

const INLINE_TOKEN = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g;

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const parts = text.split(INLINE_TOKEN).filter((part) => part.length > 0);
  return parts.map((part, index) => {
    const key = `${keyPrefix}-${index}`;
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code
          className="rounded bg-stone-200/70 px-1 py-0.5 font-mono text-[12px] text-stone-900"
          key={key}
        >
          {part.slice(1, -1)}
        </code>
      );
    }
    if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
      return (
        <strong className="font-semibold text-stone-950" key={key}>
          {part.slice(2, -2)}
        </strong>
      );
    }
    if (part.startsWith("*") && part.endsWith("*") && part.length > 2) {
      return (
        <em className="italic" key={key}>
          {part.slice(1, -1)}
        </em>
      );
    }
    const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(part);
    if (link) {
      const label = link[1];
      const href = link[2];
      if (/^https?:\/\//i.test(href)) {
        return (
          <a
            className="text-blue-700 underline decoration-blue-300 underline-offset-2"
            href={href}
            key={key}
            rel="noreferrer noopener"
            target="_blank"
          >
            {label}
          </a>
        );
      }
      return <Fragment key={key}>{label}</Fragment>;
    }
    return <Fragment key={key}>{part}</Fragment>;
  });
}

export function MarkdownContent({ content }: { content: string }) {
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (line.trimStart().startsWith("```")) {
      const language = line.trim().slice(3);
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].trimStart().startsWith("```")) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push(
        <pre
          className="my-1.5 overflow-x-auto rounded-lg border border-stone-200 bg-stone-900 px-3 py-2 text-[11px] leading-5 text-stone-100"
          key={`code-${blocks.length}`}
        >
          <code>{code.join("\n")}</code>
          {language ? <span className="sr-only">语言：{language}</span> : null}
        </pre>,
      );
      continue;
    }

    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      const className =
        level === 1
          ? "mt-2 text-[14px] font-semibold text-stone-950"
          : level === 2
            ? "mt-2 text-[13px] font-semibold text-stone-900"
            : "mt-1.5 text-[12px] font-semibold text-stone-800";
      blocks.push(
        <p className={className} key={`h-${blocks.length}`}>
          {renderInline(heading[2], `h-${blocks.length}`)}
        </p>,
      );
      index += 1;
      continue;
    }

    const unordered = /^\s*[-*+]\s+(.*)$/.exec(line);
    const ordered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (unordered || ordered) {
      const items: string[] = [];
      const pattern = unordered ? /^\s*[-*+]\s+(.*)$/ : /^\s*\d+[.)]\s+(.*)$/;
      while (index < lines.length) {
        const match = pattern.exec(lines[index]);
        if (!match) break;
        items.push(match[1]);
        index += 1;
      }
      const ListTag = unordered ? "ul" : "ol";
      blocks.push(
        <ListTag
          className={`my-1.5 space-y-0.5 pl-5 text-[13px] leading-5 ${unordered ? "list-disc" : "list-decimal"}`}
          key={`list-${blocks.length}`}
        >
          {items.map((item, itemIndex) => (
            <li key={itemIndex}>{renderInline(item, `li-${blocks.length}-${itemIndex}`)}</li>
          ))}
        </ListTag>,
      );
      continue;
    }

    if (line.trim() === "") {
      index += 1;
      continue;
    }

    const paragraph: string[] = [line];
    index += 1;
    while (index < lines.length) {
      const next = lines[index];
      if (
        next.trim() === "" ||
        next.trimStart().startsWith("```") ||
        /^#{1,3}\s+/.test(next) ||
        /^\s*[-*+]\s+/.test(next) ||
        /^\s*\d+[.)]\s+/.test(next)
      ) {
        break;
      }
      paragraph.push(next);
      index += 1;
    }
    blocks.push(
      <p className="my-1 whitespace-pre-wrap break-words" key={`p-${blocks.length}`}>
        {renderInline(paragraph.join("\n"), `p-${blocks.length}`)}
      </p>,
    );
  }

  return <>{blocks}</>;
}
