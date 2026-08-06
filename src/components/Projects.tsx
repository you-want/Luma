import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listProjects } from '../lib/tauri'
import { formatBytes, formatNumber } from '../lib/format'
import { errorMessage } from '../lib/errors'
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

// Project identification runs several passes over the whole file index, which
// on a large scan (hundreds of thousands of files) is slow. It is therefore
// triggered explicitly by the user rather than on mount, so opening a result
// never freezes the UI. The same reasoning applies to duplicate detection.
type State =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'ready'; projects: ProjectCandidate[] }

export default function Projects({ scanId }: ProjectsProps) {
  const { t } = useTranslation()
  const [state, setState] = useState<State>({ status: 'idle' })

  // A new scan invalidates any previously identified projects.
  useEffect(() => {
    setState({ status: 'idle' })
  }, [scanId])

  async function handleIdentify() {
    setState({ status: 'loading' })
    try {
      const projects = await listProjects(scanId)
      setState({ status: 'ready', projects })
    } catch (error) {
      setState({ status: 'error', message: errorMessage(error) })
    }
  }

  return (
    <section className="result-section projects-section" aria-labelledby="projects-title">
      <div className="section-heading compact-heading">
        <h2 id="projects-title">{t('projects.title')}</h2>
      </div>

      {state.status === 'idle' && (
        <div className="projects-cta">
          <p className="empty-inline">{t('projects.cta')}</p>
          <button type="button" className="primary-button" onClick={handleIdentify}>
            {t('projects.identify')}
          </button>
        </div>
      )}

      {state.status === 'loading' && (
        <p className="empty-inline">{t('projects.identifying')}</p>
      )}

      {state.status === 'error' && (
        <p className="empty-inline">{state.message}</p>
      )}

      {state.status === 'ready' &&
        (state.projects.length === 0 ? (
          <p className="empty-inline">{t('projects.none')}</p>
        ) : (
          <>
            <div className="projects-summary">
              {t('projects.summary', {
                count: formatNumber(state.projects.length),
                size: formatBytes(
                  state.projects.reduce((sum, p) => sum + p.sizeBytes, 0),
                ),
              })}
            </div>

            <div className="project-list">
              {state.projects.map((project, idx) => (
                <div key={idx} className="project-card">
                  <div className="project-icon">
                    {PROJECT_ICONS[project.kind] || '📁'}
                  </div>
                  <div className="project-info">
                    <div className="project-name">{project.name}</div>
                    <div className="project-meta">
                      {t('projects.meta', {
                        kind: PROJECT_LABELS[project.kind] || project.kind,
                        files: formatNumber(project.fileCount),
                        size: formatBytes(project.sizeBytes),
                      })}
                    </div>
                    <div className="project-path">{project.path}</div>
                  </div>
                </div>
              ))}
            </div>
          </>
        ))}
    </section>
  )
}
