// Copies the files the site serves from the repository root into public/ (see .gitignore).
import { cpSync, copyFileSync, mkdirSync, rmSync } from 'node:fs'

mkdirSync('public', { recursive: true })
copyFileSync('../install.sh', 'public/install.sh')
copyFileSync('../assets/logo.svg', 'public/logo.svg')
rmSync('public/screenshots', { recursive: true, force: true })
cpSync('../docs/screenshots', 'public/screenshots', { recursive: true })
