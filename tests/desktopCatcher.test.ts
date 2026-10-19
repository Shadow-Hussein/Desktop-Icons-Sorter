import { mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { DesktopCatcher, isCatchable } from '../src/main/desktopCatcher'

describe('isCatchable', () => {
  it('takes ordinary items', () => {
    expect(isCatchable('Steam.lnk')).toBe(true)
    expect(isCatchable('report.pdf')).toBe(true)
    expect(isCatchable('Photos')).toBe(true)
  })
  it('skips partial downloads, temp and system files', () => {
    expect(isCatchable('setup.exe.crdownload')).toBe(false)
    expect(isCatchable('movie.mkv.part')).toBe(false)
    expect(isCatchable('x.TMP')).toBe(false)
    expect(isCatchable('~$report.docx')).toBe(false)
    expect(isCatchable('desktop.ini')).toBe(false)
  })
})

describe('DesktopCatcher', () => {
  const SETTLE = 60
  let dir: string
  let caught: string[]
  let catcher: DesktopCatcher

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'sorter-desktop-'))
    writeFileSync(join(dir, 'existing.txt'), 'x')
    caught = []
    catcher = new DesktopCatcher(
      dir,
      async (path) => {
        caught.push(basename(path))
        return true
      },
      SETTLE
    )
    catcher.start()
  })
  afterEach(() => {
    catcher.stop()
    rmSync(dir, { recursive: true, force: true })
  })

  const settle = (): Promise<void> => new Promise((r) => setTimeout(r, SETTLE * 6))

  it('hands over new items once they settle', async () => {
    writeFileSync(join(dir, 'new.txt'), 'hello')
    await settle()
    expect(caught).toEqual(['new.txt'])
  })

  it('catches a finished download but not its partial file', async () => {
    writeFileSync(join(dir, 'setup.exe.crdownload'), 'data')
    await settle()
    renameSync(join(dir, 'setup.exe.crdownload'), join(dir, 'setup.exe'))
    await settle()
    expect(caught).toEqual(['setup.exe'])
  })

  it('leaves renamed items, empty new items and ignored names alone', async () => {
    renameSync(join(dir, 'existing.txt'), join(dir, 'renamed.txt'))
    writeFileSync(join(dir, 'empty.txt'), '')
    mkdirSync(join(dir, 'New folder'))
    catcher.ignore('sent-back.txt')
    writeFileSync(join(dir, 'sent-back.txt'), 'x')
    await settle()
    expect(caught).toEqual([])
  })

  it('hands over a copied item once, and catches the next new one', async () => {
    // The default handler leaves the item in place, as copying does.
    writeFileSync(join(dir, 'one.txt'), 'x')
    await settle()
    writeFileSync(join(dir, 'two.txt'), 'x')
    await settle()
    expect(caught).toEqual(['one.txt', 'two.txt'])
  })

  it('stops watching when stopped', async () => {
    catcher.stop()
    writeFileSync(join(dir, 'late.txt'), 'x')
    await settle()
    expect(caught).toEqual([])
  })
})
