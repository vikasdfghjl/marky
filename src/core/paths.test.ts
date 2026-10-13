import { describe, expect, it } from 'vitest';
import { fileName, folderOf, joinWindowsPath, UNTITLED } from './paths';

describe('joinWindowsPath', () => {
  it('joins under a drive folder', () => {
    expect(joinWindowsPath('C:\\docs', 'a.md')).toBe('C:\\docs\\a.md');
    expect(joinWindowsPath('C:\\docs', 'img\\pic.png')).toBe('C:\\docs\\img\\pic.png');
  });

  it('normalises . and .. without climbing above the drive root', () => {
    expect(joinWindowsPath('C:\\docs\\sub', '..\\a.md')).toBe('C:\\docs\\a.md');
    expect(joinWindowsPath('C:\\docs', '..\\..\\Windows\\win.ini')).toBe('C:\\Windows\\win.ini');
    expect(joinWindowsPath('C:\\', '..\\secret')).toBe('C:\\secret');
  });

  it('joins under a UNC share', () => {
    expect(joinWindowsPath('\\\\server\\share\\docs', 'a.md')).toBe('\\\\server\\share\\docs\\a.md');
    expect(joinWindowsPath('\\\\server\\share\\docs', '..\\a.md')).toBe('\\\\server\\share\\a.md');
  });
});

describe('fileName and folderOf', () => {
  it('handles null and Windows paths', () => {
    expect(fileName(null)).toBe(UNTITLED);
    expect(fileName('C:\\a\\b.md')).toBe('b.md');
    expect(folderOf('C:\\a\\b.md')).toBe('C:\\a');
    expect(folderOf(null)).toBeNull();
  });
});
