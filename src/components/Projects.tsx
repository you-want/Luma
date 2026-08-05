import { useEffect, useState } from 'react'
import { listProjects } from '../lib/tauri'
import type { ProjectCandidate } from '../types/scan'

interface ProjectsProps {
  scanId: string
}

const PROJECT_LABELS: Record<string, string> = {
  nodejs: 'Node.js',
  rust: 'Rust',
  python: 'Python',
  git: 'Git',
  xcode: 'Xcode',
  maven: 'Maven',
  gradle: 'Gradle',
}

const PROJECT_ICONS: Record<string, string> = {
  nodejs: '📦',
  rust: '🦀',
  python: '🐍',
  git: '📁',
  xcode: '🔨',
  maven: '☕',
  gradle: '🐘',
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

function formatCount(count: number): string {
  if (count < 1000) return count.toString()
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`
  return `${Math.floor(count / 1000)}k`
}

export default function Projects({ scanId }: ProjectsProps) {
  const [projects, setProjects] = useState<ProjectCandidate[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    listProjects(scanId)
      .then(setProjects)
      .finally(() => setLoading(false))
  }, [scanId])

  if (loading) {
    return (
      <div className="projects-section">
        <p style={{ color: 'var(--text-secondary)', fontSize: '13px' }}>
          正在识别开发项目...
        </p>
      </div>
    )
  }

  if (projects.length === 0) {
    return (
      <div className="projects-section">
        <p style={{ color: 'var(--text-secondary)', fontSize: '13px' }}>
          未检测到开发项目目录
        </p>
      </div>
    )
  }

  return (
    <div className="projects-section">
      <div className="projects-summary">
        发现 <strong>{projects.length}</strong> 个开发项目，共占用{' '}
        <strong>
          {formatBytes(projects.reduce((sum, p) => sum + p.sizeBytes, 0))}
        </strong>
      </div>

      <div className="project-list">
        {projects.map((project, idx) => (
          <div key={idx} className="project-card">
            <div className="project-icon">
              {PROJECT_ICONS[project.kind] || '📁'}
            </div>
            <div className="project-info">
              <div className="project-name">{project.name}</div>
              <div className="project-meta">
                {PROJECT_LABELS[project.kind] || project.kind} ·{' '}
                {formatCount(project.fileCount)} 文件 ·{' '}
                {formatBytes(project.sizeBytes)}
              </div>
              <div className="project-path">{project.path}</div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
