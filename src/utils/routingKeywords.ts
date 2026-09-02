/**
 * Keyword lists shared by the free pre-gates: the report/code turn-mode
 * hint (turnModeClassifier) and the routing task/difficulty gate
 * (routingTaskClassifier). One list, so the hint and the gate can never
 * disagree on what code looks like.
 */
export const CODE_KW = [
  "code", "function", "bug", "debug", "error", "exception", "compile", "script",
  "program", "implement", "refactor", "snippet", "fix this", "rewrite this",
  "api", "regex", "algorithm", "stack trace", "syntax", "variable",
  "python", "javascript", "typescript", "rust", "java", "c++", "sql", "html",
  "css", "git", "terminal", "shell", "npm", "docker", "class ", "def ", "import ",
];
export const MATH_KW = [
  "calculate", "equation", "integral", "derivative", "solve for", "math",
  "probability", "matrix", "algebra", "geometry", "theorem", "factor", "prime",
  "percentage", "compute ",
];
export const REASONING_KW = [
  "step by step", "step-by-step", "reason", "analyze", "analyse", "plan ",
  "strategy", "logic", "prove", "deduce", "trade-off", "tradeoff", "compare ",
  "pros and cons", "figure out", "work out", "puzzle", "design ", "architect",
  "optimize", "optimise",
];
