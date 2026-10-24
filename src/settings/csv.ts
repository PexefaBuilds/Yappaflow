// Minimal RFC 4180 CSV: quoted fields may hold commas, quotes ("") and line breaks.

// A leading ' stops spreadsheet apps running "=..." cells as formulas; parseCsv strips it again.
const guard = (f: string) => (/^[=+\-@]/.test(f) ? `'${f}` : f);

export function toCsv(rows: string[][]): string {
  return rows.map((r) => r.map(guard).map((f) => (/[",\r\n]/.test(f) ? `"${f.replace(/"/g, '""')}"` : f)).join(",")).join("\r\n") + "\r\n";
}

export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [], field = "", quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') { field += '"'; i++; }
      else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"') quoted = true;
    else if (c === ",") { row.push(field); field = ""; }
    else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field); rows.push(row); row = []; field = "";
    } else field += c;
  }
  if (field || row.length) { row.push(field); rows.push(row); }
  return rows.filter((r) => r.some((f) => f.trim())).map((r) => r.map((f) => f.replace(/^'(?=[=+\-@])/, "")));
}

export function download(name: string, text: string, type = "text/csv") {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([text], { type: `${type};charset=utf-8` }));
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

/** Opens the file picker and resolves with the chosen file's rows (header row dropped when it matches). */
export function pickCsv(header: string): Promise<string[][]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".csv,text/csv";
    input.onchange = async () => {
      const f = input.files?.[0];
      if (!f) return resolve([]);
      const rows = parseCsv(await f.text());
      if (rows[0]?.[0]?.trim().toLowerCase() === header) rows.shift();
      resolve(rows);
    };
    input.click();
  });
}
