import { useState, useRef } from 'react'
import {
  motion,
  useScroll,
  useTransform,
  useInView,
  AnimatePresence,
} from 'framer-motion'

/* ───────────────────────────────────────────────────
   Data
   ─────────────────────────────────────────────────── */

const FEATURES = [
  {
    icon: '⚙️',
    title: 'State Machines',
    desc: 'Declare states and transitions once. The kernel enforces every valid path — no ad-hoc if/else chains.',
    color: '#10B981',
    bg: '#ECFDF5',
  },
  {
    icon: '🛡️',
    title: 'Role-Based Access',
    desc: 'Fine-grained RBAC per resource type. Every operation checks actor permissions at the kernel boundary.',
    color: '#059669',
    bg: '#D1FAE5',
  },
  {
    icon: '📋',
    title: 'Policy Engine',
    desc: 'Compliance rules evaluated on every transition. Policies can allow, deny, or escalate — with full context.',
    color: '#F59E0B',
    bg: '#FFFBEB',
  },
  {
    icon: '🔒',
    title: 'Invariant Checks',
    desc: 'Strong and soft mode data integrity. Invariants run before and after mutations — violated? The kernel rejects.',
    color: '#FB7185',
    bg: '#FFF1F2',
  },
  {
    icon: '📜',
    title: 'Audit Trails',
    desc: 'Immutable event log per resource. Every state change, policy evaluation, and agent recommendation is recorded.',
    color: '#34D399',
    bg: '#ECFDF5',
  },
  {
    icon: '🔗',
    title: 'Graph Relationships',
    desc: 'First-class resource graph with typed edges. Aggregate fields across neighbors — exposure limits, cross-references.',
    color: '#6EE7B7',
    bg: '#F0FDF4',
  },
]

const CODE_TABS = [
  {
    label: 'Define',
    title: 'Declare Your Domain',
    desc: 'Define states, transitions, roles, policies, and invariants in a single function call. The kernel handles the rest.',
    code: `<span class="keyword">from</span> reconcile <span class="keyword">import</span> define_system, PolicyResult

<span class="comment"># Define the entire governance model declaratively</span>
loan_os = <span class="function">define_system</span>(
    name=<span class="string">"loan"</span>,
    states=[
        <span class="string">"DRAFT"</span>, <span class="string">"APPLIED"</span>, <span class="string">"UNDERWRITING"</span>,
        <span class="string">"APPROVED"</span>, <span class="string">"DISBURSED"</span>, <span class="string">"CLOSED"</span>,
    ],
    transitions=[
        (<span class="string">"DRAFT"</span>, <span class="string">"APPLIED"</span>),
        (<span class="string">"APPLIED"</span>, <span class="string">"UNDERWRITING"</span>),
        (<span class="string">"UNDERWRITING"</span>, <span class="string">"APPROVED"</span>),
        (<span class="string">"APPROVED"</span>, <span class="string">"DISBURSED"</span>),
        (<span class="string">"DISBURSED"</span>, <span class="string">"CLOSED"</span>),
    ],
    roles={
        <span class="string">"data_entry"</span>: [<span class="string">"view"</span>, <span class="string">"transition:APPLIED"</span>],
        <span class="string">"underwriter"</span>: [<span class="string">"view"</span>, <span class="string">"transition:APPROVED"</span>],
        <span class="string">"branch_mgr"</span>: [<span class="string">"view"</span>, <span class="string">"transition:*"</span>],
    },
)`,
  },
  {
    label: 'Create',
    title: 'Create Resources',
    desc: 'Instantiate resources with typed data. The kernel validates invariants and places the resource in its initial state.',
    code: `<span class="comment"># Create a new loan resource</span>
loan = loan_os.<span class="function">create</span>(
    {
        <span class="string">"amount"</span>: <span class="number">800_000</span>,
        <span class="string">"purpose"</span>: <span class="string">"working_capital"</span>,
        <span class="string">"interest_rate"</span>: <span class="number">12.5</span>,
        <span class="string">"bureau_score"</span>: <span class="number">742</span>,
    },
    actor=<span class="string">"maker-1"</span>,
)

<span class="builtin">print</span>(loan.resource.id)      <span class="comment"># uuid</span>
<span class="builtin">print</span>(loan.resource.state)   <span class="comment"># "DRAFT"</span>`,
  },
  {
    label: 'Transition',
    title: 'State Transitions',
    desc: 'Request state changes through the kernel. Policies, invariants, and permissions are enforced automatically.',
    code: `<span class="comment"># Move through the workflow</span>
loan_os.<span class="function">transition</span>(
    loan.resource.id,
    <span class="string">"APPLIED"</span>,
    actor=<span class="string">"maker-1"</span>,
    role=<span class="string">"data_entry"</span>,
)

<span class="comment"># The kernel checks:</span>
<span class="comment">#   ✓ DRAFT → APPLIED is a valid transition</span>
<span class="comment">#   ✓ "data_entry" has "transition:APPLIED"</span>
<span class="comment">#   ✓ All invariants hold</span>
<span class="comment">#   ✓ All policies allow</span>`,
  },
  {
    label: 'Project',
    title: 'Interface Projections',
    desc: 'Compute role-scoped views of any resource. Each role sees only what governance allows — available actions, filtered data.',
    code: `<span class="comment"># What does the underwriter see?</span>
projection = loan_os.<span class="function">project</span>(
    loan.resource.id,
    <span class="string">"underwriter"</span>,
)

<span class="builtin">print</span>(projection.to_json())
<span class="comment"># {</span>
<span class="comment">#   "state": "APPLIED",</span>
<span class="comment">#   "available_transitions": ["APPROVED"],</span>
<span class="comment">#   "data": { "amount": 800000, ... },</span>
<span class="comment">#   "policy_results": [...],</span>
<span class="comment">#   "agent_recommendations": [...]</span>
<span class="comment"># }</span>`,
  },
]

const ARCH_STEPS = [
  {
    icon: '📝',
    title: 'Define',
    desc: 'Declare states, roles, policies, invariants in Python',
  },
  {
    icon: '⚡',
    title: 'Kernel',
    desc: 'Rust core validates every operation at O(1) cost',
  },
  {
    icon: '🔐',
    title: 'Enforce',
    desc: 'Policies, RBAC, invariants checked on every transition',
  },
  {
    icon: '📊',
    title: 'Project',
    desc: 'Role-scoped views with available actions and filtered data',
  },
]

const ADVANCED = [
  {
    icon: '🤖',
    title: 'GovernedLLM',
    desc: 'Wrap any LLM with the governance constraint protocol. Every AI action routes through the kernel — the LLM cannot bypass policies.',
    code: `<span class="keyword">from</span> reconcile <span class="keyword">import</span> GovernedLLM

llm = <span class="function">GovernedLLM</span>(
    system, actor=<span class="string">"ai-agent"</span>,
    role=<span class="string">"underwriter"</span>,
)
result = llm.<span class="function">interact</span>(
    resource_id, <span class="string">"Approve this loan"</span>
)`,
  },
  {
    icon: '🏢',
    title: 'Multi-Tenant Platform',
    desc: 'Run multiple isolated apps on a single instance. Each app gets its own kernel, types, roles, and policies.',
    code: `<span class="keyword">from</span> reconcile <span class="keyword">import</span> ReconcilePlatform

platform = <span class="function">ReconcilePlatform</span>()
platform.<span class="function">register_app</span>(<span class="string">"bank_a"</span>, system_a)
platform.<span class="function">register_app</span>(<span class="string">"bank_b"</span>, system_b)
<span class="comment"># Complete isolation</span>`,
  },
  {
    icon: '🚀',
    title: 'FastAPI Adapters',
    desc: 'One-line HTTP API. Single-system or multi-app — the adapter generates typed endpoints from your system spec.',
    code: `<span class="keyword">from</span> reconcile.api <span class="keyword">import</span> create_app
<span class="keyword">from</span> reconcile.examples <span class="keyword">import</span> (
    create_loan_operating_system
)

app = <span class="function">create_app</span>(
    create_loan_operating_system().native
)`,
  },
]

/* ───────────────────────────────────────────────────
   Animation variants
   ─────────────────────────────────────────────────── */

const EASE_OUT: [number, number, number, number] = [0.25, 0.46, 0.45, 0.94]

const fadeUp = {
  hidden: { opacity: 0, y: 30 },
  visible: (i: number = 0) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.1, duration: 0.6, ease: EASE_OUT },
  }),
}

const fadeIn = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: { duration: 0.6 } },
}

const scaleIn = {
  hidden: { opacity: 0, scale: 0.9 },
  visible: (i: number = 0) => ({
    opacity: 1,
    scale: 1,
    transition: { delay: i * 0.12, duration: 0.5, ease: EASE_OUT },
  }),
}

/* ───────────────────────────────────────────────────
   Reusable components
   ─────────────────────────────────────────────────── */

function SectionHeader({
  label,
  title,
  description,
}: {
  label: string
  title: string
  description?: string
}) {
  const ref = useRef(null)
  const inView = useInView(ref, { once: true, margin: '-80px' })

  return (
    <motion.div ref={ref} initial="hidden" animate={inView ? 'visible' : 'hidden'}>
      <motion.span className="section-label" variants={fadeUp}>
        {label}
      </motion.span>
      <motion.h2 variants={fadeUp} custom={1}>
        {title}
      </motion.h2>
      {description && (
        <motion.p className="section-description" variants={fadeUp} custom={2}>
          {description}
        </motion.p>
      )}
    </motion.div>
  )
}

/* ───────────────────────────────────────────────────
   Floating Nodes (Hero background)
   ─────────────────────────────────────────────────── */

function FloatingNodes() {
  const nodes = Array.from({ length: 18 }, (_, i) => ({
    id: i,
    x: Math.random() * 100,
    y: Math.random() * 100,
    size: 4 + Math.random() * 8,
    duration: 6 + Math.random() * 8,
    delay: Math.random() * 4,
  }))

  return (
    <div className="hero-nodes">
      {nodes.map((n) => (
        <motion.div
          key={n.id}
          className="floating-node"
          style={{
            left: `${n.x}%`,
            top: `${n.y}%`,
            width: n.size,
            height: n.size,
          }}
          animate={{
            y: [-20, 20, -20],
            opacity: [0.08, 0.2, 0.08],
          }}
          transition={{
            duration: n.duration,
            delay: n.delay,
            repeat: Infinity,
            ease: 'easeInOut',
          }}
        />
      ))}
    </div>
  )
}

/* ───────────────────────────────────────────────────
   Motion Background Blobs
   ─────────────────────────────────────────────────── */

function MotionBackground({ colors }: { colors: string[] }) {
  const blobs = colors.map((color, i) => ({
    color,
    x: 15 + i * 30,
    y: 20 + (i % 2) * 40,
    size: 200 + i * 80,
    dx: i % 2 === 0 ? 40 : -40,
    dy: i % 2 === 0 ? -30 : 30,
    duration: 12 + i * 4,
  }))

  return (
    <div className="motion-bg">
      {blobs.map((b, i) => (
        <motion.div
          key={i}
          className="motion-blob"
          style={{
            background: b.color,
            width: b.size,
            height: b.size,
            left: `${b.x}%`,
            top: `${b.y}%`,
          }}
          animate={{
            x: [0, b.dx, 0],
            y: [0, b.dy, 0],
            scale: [1, 1.15, 1],
          }}
          transition={{
            duration: b.duration,
            repeat: Infinity,
            ease: 'easeInOut',
          }}
        />
      ))}
    </div>
  )
}

/* ───────────────────────────────────────────────────
   Nav
   ─────────────────────────────────────────────────── */

function Nav() {
  return (
    <motion.nav
      className="nav"
      initial={{ y: -64 }}
      animate={{ y: 0 }}
      transition={{ duration: 0.5, ease: 'easeOut' }}
    >
      <div className="nav-inner">
        <a href="#" className="nav-logo">
          <span className="nav-logo-icon">R</span>
          Reconcile
        </a>
        <ul className="nav-links">
          <li><a href="#features">Features</a></li>
          <li><a href="#code">Code</a></li>
          <li><a href="#architecture">Architecture</a></li>
          <li><a href="#advanced">Advanced</a></li>
        </ul>
        <div className="nav-cta">
          <a
            href="https://github.com/nikitph/reconcile"
            target="_blank"
            rel="noopener noreferrer"
            className="btn btn-secondary"
            style={{ padding: '8px 18px', fontSize: '0.85rem' }}
          >
            ⭐ GitHub
          </a>
        </div>
      </div>
    </motion.nav>
  )
}

/* ───────────────────────────────────────────────────
   Hero
   ─────────────────────────────────────────────────── */

function Hero() {
  const [copied, setCopied] = useState(false)
  const ref = useRef(null)
  const { scrollYProgress } = useScroll({ target: ref, offset: ['start start', 'end start'] })
  const y = useTransform(scrollYProgress, [0, 1], [0, 200])
  const opacity = useTransform(scrollYProgress, [0, 0.8], [1, 0])

  const handleCopy = () => {
    navigator.clipboard.writeText('pip install reconcile')
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <section className="hero" ref={ref}>
      <div className="hero-bg">
        <div className="hero-grid" />
        <motion.div className="hero-glow hero-glow-1" animate={{ scale: [1, 1.2, 1], x: [0, 30, 0] }} transition={{ duration: 8, repeat: Infinity, ease: 'easeInOut' }} />
        <motion.div className="hero-glow hero-glow-2" animate={{ scale: [1.2, 1, 1.2], x: [0, -20, 0] }} transition={{ duration: 10, repeat: Infinity, ease: 'easeInOut' }} />
        <motion.div className="hero-glow hero-glow-3" animate={{ scale: [1, 1.3, 1], y: [0, 20, 0] }} transition={{ duration: 12, repeat: Infinity, ease: 'easeInOut' }} />
        <FloatingNodes />
      </div>

      <motion.div className="hero-content" style={{ y, opacity }}>
        <motion.div
          className="hero-badge"
          initial={{ opacity: 0, scale: 0.8 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ duration: 0.5 }}
        >
          <motion.span
            className="hero-badge-dot"
            animate={{ scale: [1, 1.3, 1] }}
            transition={{ duration: 2, repeat: Infinity }}
          />
          Rust core · Python surface · v0.1
        </motion.div>

        <motion.h1
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.1 }}
        >
          Governance Runtime for{' '}
          <span className="gradient-text">Enterprise Workflows</span>
        </motion.h1>

        <motion.p
          className="hero-subtitle"
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.25 }}
        >
          Define your domain model once. Reconcile enforces state machines, RBAC,
          policies, invariants, and audit trails — through a single kernel.
        </motion.p>

        <motion.div
          className="hero-actions"
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.4 }}
        >
          <a href="#code" className="btn btn-primary">
            See How It Works
          </a>
          <a
            href="https://github.com/nikitph/reconcile"
            target="_blank"
            rel="noopener noreferrer"
            className="btn btn-secondary"
          >
            View on GitHub →
          </a>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.55 }}
          onClick={handleCopy}
          className="install-badge"
          style={{ margin: '0 auto' }}
        >
          <span className="install-badge-prefix">$</span>
          pip install reconcile
          <span className="install-badge-copy">{copied ? '✓ Copied!' : 'Click to copy'}</span>
        </motion.div>
      </motion.div>
    </section>
  )
}

/* ───────────────────────────────────────────────────
   Features
   ─────────────────────────────────────────────────── */

function Features() {
  const ref = useRef(null)
  const inView = useInView(ref, { once: true, margin: '-100px' })

  return (
    <section className="features" id="features" style={{ position: 'relative' }}>
      <MotionBackground colors={['#10B981', '#34D399', '#6EE7B7']} />
      <div className="container">
        <SectionHeader
          label="Core Primitives"
          title="Everything the Kernel Enforces"
          description="Six foundational guarantees, checked on every single operation. No opt-out, no shortcuts."
        />

        <div className="features-grid" ref={ref}>
          {FEATURES.map((f, i) => (
            <motion.div
              key={f.title}
              className="feature-card"
              style={{ '--card-accent': f.color } as React.CSSProperties}
              variants={scaleIn}
              initial="hidden"
              animate={inView ? 'visible' : 'hidden'}
              custom={i}
              whileHover={{ y: -4, transition: { duration: 0.2 } }}
            >
              <div
                className="feature-icon"
                style={{ background: f.bg, color: f.color }}
              >
                {f.icon}
              </div>
              <h3>{f.title}</h3>
              <p>{f.desc}</p>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  )
}

/* ───────────────────────────────────────────────────
   Code Section
   ─────────────────────────────────────────────────── */

function CodeSection() {
  const [activeTab, setActiveTab] = useState(0)
  const ref = useRef(null)
  const inView = useInView(ref, { once: true, margin: '-100px' })

  return (
    <section className="code-section" id="code" ref={ref} style={{ position: 'relative' }}>
      <MotionBackground colors={['#10B981', '#FB7185', '#34D399']} />
      <div className="container">
        <SectionHeader
          label="Developer Experience"
          title="A 50-Line Enterprise System"
          description="Define an entire loan origination workflow with state machines, RBAC, policies, and invariants — in pure Python."
        />

        <motion.div
          className="code-layout"
          variants={fadeIn}
          initial="hidden"
          animate={inView ? 'visible' : 'hidden'}
        >
          <div className="code-explanation">
            <div className="step-list">
              {CODE_TABS.map((tab, i) => (
                <motion.div
                  key={tab.label}
                  className={`step-item ${i === activeTab ? 'active' : ''}`}
                  onClick={() => setActiveTab(i)}
                  whileHover={{ x: 4 }}
                  whileTap={{ scale: 0.98 }}
                >
                  <span className="step-number">{i + 1}</span>
                  <div className="step-text">
                    <h4>{tab.title}</h4>
                    <p>{tab.desc}</p>
                  </div>
                </motion.div>
              ))}
            </div>
          </div>

          <div>
            <div className="code-tabs">
              {CODE_TABS.map((tab, i) => (
                <motion.button
                  key={tab.label}
                  className={`code-tab ${i === activeTab ? 'active' : ''}`}
                  onClick={() => setActiveTab(i)}
                  whileHover={{ scale: 1.02 }}
                  whileTap={{ scale: 0.98 }}
                >
                  {tab.label}
                </motion.button>
              ))}
            </div>
            <AnimatePresence mode="wait">
              <motion.div
                key={activeTab}
                className="code-block"
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -8 }}
                transition={{ duration: 0.3 }}
              >
                <pre dangerouslySetInnerHTML={{ __html: CODE_TABS[activeTab].code }} />
              </motion.div>
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </section>
  )
}

/* ───────────────────────────────────────────────────
   Architecture
   ─────────────────────────────────────────────────── */

function Architecture() {
  const ref = useRef(null)
  const inView = useInView(ref, { once: true, margin: '-100px' })

  return (
    <section className="architecture" id="architecture" ref={ref} style={{ position: 'relative' }}>
      <MotionBackground colors={['#34D399', '#6EE7B7']} />
      <div className="container">
        <SectionHeader
          label="How It Works"
          title="Define → Kernel → Enforce → Project"
          description="Every operation flows through the Rust kernel.
            No governance bypass is architecturally possible."
        />

        <div className="arch-pipeline">
          {ARCH_STEPS.map((step, i) => (
            <motion.div
              key={step.title}
              className="arch-step"
              variants={fadeUp}
              initial="hidden"
              animate={inView ? 'visible' : 'hidden'}
              custom={i}
              whileHover={{ y: -4, transition: { duration: 0.2 } }}
            >
              <div className="arch-icon">{step.icon}</div>
              <h3>{step.title}</h3>
              <p>{step.desc}</p>
            </motion.div>
          ))}
        </div>

        <motion.div
          style={{ textAlign: 'center', marginTop: 32 }}
          variants={fadeUp}
          initial="hidden"
          animate={inView ? 'visible' : 'hidden'}
          custom={5}
        >
          <span className="arch-rust-badge">
            🦀 Core runtime in Rust via PyO3 &nbsp;·&nbsp; Python surface via maturin
          </span>
        </motion.div>
      </div>
    </section>
  )
}

/* ───────────────────────────────────────────────────
   Advanced
   ─────────────────────────────────────────────────── */

function Advanced() {
  const ref = useRef(null)
  const inView = useInView(ref, { once: true, margin: '-100px' })

  return (
    <section className="advanced" id="advanced" ref={ref} style={{ position: 'relative' }}>
      <MotionBackground colors={['#10B981', '#059669', '#FB7185']} />
      <div className="container">
        <SectionHeader
          label="Beyond the Basics"
          title="AI Governance, Multi-Tenancy, HTTP in One Line"
          description="Reconcile extends beyond state machines — govern LLM agents, isolate tenants, and ship APIs instantly."
        />

        <div className="advanced-grid">
          {ADVANCED.map((card, i) => (
            <motion.div
              key={card.title}
              className="advanced-card"
              variants={scaleIn}
              initial="hidden"
              animate={inView ? 'visible' : 'hidden'}
              custom={i}
              whileHover={{ y: -4, transition: { duration: 0.2 } }}
            >
              <div className="advanced-card-icon">{card.icon}</div>
              <h3>{card.title}</h3>
              <p>{card.desc}</p>
              <div className="advanced-code">
                <pre dangerouslySetInnerHTML={{ __html: card.code }} />
              </div>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  )
}

/* ───────────────────────────────────────────────────
   Footer
   ─────────────────────────────────────────────────── */

function Footer() {
  return (
    <motion.footer
      initial={{ opacity: 0 }}
      whileInView={{ opacity: 1 }}
      viewport={{ once: true }}
      transition={{ duration: 0.6 }}
    >
      <div className="footer-inner">
        <div className="footer-tech">
          <span className="footer-tech-badge">🦀 Rust</span>
          <span className="footer-tech-badge">🐍 Python 3.11+</span>
          <span className="footer-tech-badge">⚡ PyO3 + Maturin</span>
        </div>

        <div className="footer-links">
          <a href="https://github.com/nikitph/reconcile" target="_blank" rel="noopener noreferrer">GitHub</a>
          <a href="https://pypi.org/project/reconcile/" target="_blank" rel="noopener noreferrer">PyPI</a>
          <a href="https://github.com/nikitph/reconcile/issues" target="_blank" rel="noopener noreferrer">Issues</a>
          <a href="https://github.com/nikitph/reconcile/blob/main/README.md" target="_blank" rel="noopener noreferrer">Docs</a>
        </div>

        <p className="footer-copy">
          © {new Date().getFullYear()} Reconcile · Governance runtime for enterprise workflows
        </p>
      </div>
    </motion.footer>
  )
}

/* ───────────────────────────────────────────────────
   App
   ─────────────────────────────────────────────────── */

export default function App() {
  return (
    <>
      <Nav />
      <Hero />
      <Features />
      <CodeSection />
      <Architecture />
      <Advanced />
      <Footer />
    </>
  )
}
