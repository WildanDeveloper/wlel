/// Source position: 1-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// Source span covering [start, end). Diagnostics print `start` as `line:col`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    pub fn new(start_line: usize, start_col: usize, end_line: usize, end_col: usize) -> Span {
        Span {
            start: Pos { line: start_line, col: start_col },
            end: Pos { line: end_line, col: end_col },
        }
    }

    /// Zero-width span at one position.
    pub fn point(line: usize, col: usize) -> Span {
        Span::new(line, col, line, col)
    }

    /// Smallest span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start, end: other.end }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.start.line, self.start.col)
    }
}

/// An AST node paired with the source span it came from.
/// Equality ignores the span so tests can compare structure only.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Spanned<T> {
        Spanned { node, span }
    }
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_display() {
        let s = Span::new(12, 5, 12, 9);
        assert_eq!(s.to_string(), "12:5");
    }

    #[test]
    fn span_union_takes_other_end() {
        let a = Span::new(1, 1, 1, 4);
        let b = Span::new(3, 2, 3, 8);
        assert_eq!(a.to(b), Span::new(1, 1, 3, 8));
    }

    #[test]
    fn spanned_eq_ignores_span() {
        let a = Spanned::new(1, Span::point(1, 1));
        let b = Spanned::new(1, Span::point(9, 9));
        assert_eq!(a, b);
    }
}
