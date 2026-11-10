#include "griditem.h"

#include <QtCore/QElapsedTimer>
#include <QtCore/QPointer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QInputMethod>
#include <QtGui/QKeyEvent>
#include <QtGui/QPainter>
#include <QtQml/qqml.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <vector>

#include "kyko-spike/src/bridge.cxx.h"

namespace {
QPointer<GridItem> g_item;

const quint8 BOLD = 1, ITALIC = 2, UNDERLINE = 4, WIDE = 16, WIDE_SPACER = 32;

QColor rgb(quint32 v) { return QColor((v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff); }

void send(const QByteArray& b) {
    if (!b.isEmpty())
        spike_write(rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(b.constData()), b.size()));
}
}  // namespace

struct GridItem::Stats {
    std::vector<double> ms;
    bool full = false;
};

GridItem::GridItem(QQuickItem* parent) : QQuickPaintedItem(parent) {
    g_item = this;
    setFlag(ItemAcceptsInputMethod, true);
    setAcceptedMouseButtons(Qt::LeftButton);
    setActiveFocusOnTab(true);
    setOpaquePainting(true);

    m_font = QFont(QString::fromStdString(std::string(spike_palette_font())));
    m_font.setPointSize(spike_palette_font_size());
    m_font.setStyleHint(QFont::Monospace);
    m_font.setFixedPitch(true);
    m_font.setKerning(false);
    QFontMetricsF fm(m_font);
    const qreal natural = fm.horizontalAdvance(QLatin1Char('M'));
    m_cellW = std::ceil(natural);
    // Make every advance exactly one whole-pixel cell, so a run of text stays on the grid.
    m_font.setLetterSpacing(QFont::AbsoluteSpacing, m_cellW - natural);
    m_cellH = std::ceil(fm.lineSpacing());
    m_ascent = fm.ascent() + (m_cellH - fm.height()) / 2;
    m_bold = m_font;
    m_bold.setBold(true);

    if (spike_stats_enabled()) {
        m_stats = new Stats;
        m_stats->full = qEnvironmentVariableIsSet("KYKO_SPIKE_FULL");
    }
}

GridItem::~GridItem() {
    if (m_stats && !m_stats->ms.empty()) {
        auto& v = m_stats->ms;
        std::sort(v.begin(), v.end());
        auto at = [&](double q) { return v[std::min(v.size() - 1, size_t(q * v.size()))]; };
        std::fprintf(stderr, "paint_ms n=%zu p50=%.3f p95=%.3f max=%.3f cols=%d rows=%d\n", v.size(), at(0.50),
                     at(0.95), v.back(), m_cols, m_rows);
    }
    delete m_stats;
}

void GridItem::geometryChange(const QRectF& newGeometry, const QRectF& oldGeometry) {
    QQuickPaintedItem::geometryChange(newGeometry, oldGeometry);
    const int cols = int(newGeometry.width() / m_cellW);
    const int rows = int(newGeometry.height() / m_cellH);
    if (cols < 2 || rows < 1 || (cols == m_cols && rows == m_rows))
        return;
    m_cols = cols;
    m_rows = rows;
    if (!m_started) {
        m_started = true;
        spike_start(quint16(cols), quint16(rows));
    } else {
        spike_resize(quint16(cols), quint16(rows));
    }
    Q_EMIT gridChanged();
    update();
}

void GridItem::paint(QPainter* p) {
    QElapsedTimer timer;
    if (m_stats)
        timer.start();

    rust::Vec<FfiCell> cells;
    quint16 cols = 0, rows = 0;
    int cx = -1, cy = -1;
    spike_snapshot(cells, cols, rows, cx, cy);
    m_curX = cx;
    m_curY = cy;

    const QColor bg = rgb(spike_palette_bg());
    p->fillRect(QRectF(0, 0, width(), height()), bg);
    p->setFont(m_font);

    const int status = spike_exit_status();
    QString run;
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const qreal y = r * m_cellH;
        // Backgrounds: one rect per run of equal colour.
        for (int c = 0; c < cols;) {
            int e = c + 1;
            while (e < cols && row[e].bg == row[c].bg)
                ++e;
            if (row[c].bg != spike_palette_bg())
                p->fillRect(QRectF(c * m_cellW, y, (e - c) * m_cellW, m_cellH), rgb(row[c].bg));
            c = e;
        }
        // Text: runs of ASCII with equal colour and style; anything else cell by cell.
        for (int c = 0; c < cols;) {
            const FfiCell& cell = row[c];
            if (cell.flags & WIDE_SPACER || cell.ch == ' ' || cell.ch == 0) {
                ++c;
                continue;
            }
            const quint8 style = cell.flags & (BOLD | ITALIC | UNDERLINE);
            QFont f = (style & BOLD) ? m_bold : m_font;
            if (style & ITALIC)
                f.setItalic(true);
            if (style & UNDERLINE)
                f.setUnderline(true);
            p->setFont(f);
            p->setPen(rgb(cell.fg));
            if (cell.ch < 0x80) {
                int e = c;
                run.clear();
                while (e < cols && row[e].ch < 0x80 && row[e].ch != 0 && row[e].fg == cell.fg &&
                       (row[e].flags & (BOLD | ITALIC | UNDERLINE)) == style && !(row[e].flags & WIDE_SPACER)) {
                    run.append(QChar(ushort(row[e].ch)));
                    ++e;
                }
                p->drawText(QPointF(c * m_cellW, y + m_ascent), run);
                c = e;
            } else {
                const char32_t u = cell.ch;
                const qreal w = (cell.flags & WIDE) ? 2 * m_cellW : m_cellW;
                p->drawText(QRectF(c * m_cellW, y, w, m_cellH), Qt::AlignLeft | Qt::AlignVCenter | Qt::TextDontClip,
                            QString::fromUcs4(&u, 1));
                ++c;
            }
        }
    }

    if (status == -1 && cx >= 0 && cy >= 0 && cx < cols && cy < rows) {
        const FfiCell& cell = cells[size_t(cy) * cols + cx];
        const QRectF box(cx * m_cellW, cy * m_cellH, m_cellW, m_cellH);
        if (hasActiveFocus()) {
            p->fillRect(box, rgb(spike_palette_cursor()));
            if (cell.ch > ' ') {
                const char32_t u = cell.ch;
                p->setFont(m_font);
                p->setPen(bg);
                p->drawText(QPointF(box.x(), box.y() + m_ascent), QString::fromUcs4(&u, 1));
            }
        } else {
            p->setPen(rgb(spike_palette_cursor()));
            p->drawRect(box.adjusted(0.5, 0.5, -0.5, -0.5));
        }
    }

    if (status != -1) {
        p->setFont(m_bold);
        p->setPen(rgb(spike_palette_fg()));
        const QString msg = QStringLiteral("Shell exited (status %1)").arg(status);
        const QRectF band(0, height() / 2 - m_cellH, width(), 2 * m_cellH);
        p->fillRect(band, bg.lighter(140));
        p->drawText(band, Qt::AlignCenter, msg);
    }

    if (m_stats) {
        m_stats->ms.push_back(timer.nsecsElapsed() / 1e6);
        if (m_stats->full && status == -1)
            update();
    }
}

void GridItem::keyPressEvent(QKeyEvent* e) {
    const Qt::KeyboardModifiers m = e->modifiers();
    QByteArray out;
    auto csi = [&](const char* plain, char final) {
        // xterm: CSI 1 ; mod final when a modifier is held.
        int mod = 1 + ((m & Qt::ShiftModifier) ? 1 : 0) + ((m & Qt::AltModifier) ? 2 : 0) +
                  ((m & Qt::ControlModifier) ? 4 : 0);
        if (mod == 1)
            out = plain;
        else
            out = QByteArray("\x1b[1;") + QByteArray::number(mod) + final;
    };
    auto tilde = [&](int n) {
        int mod = 1 + ((m & Qt::ShiftModifier) ? 1 : 0) + ((m & Qt::AltModifier) ? 2 : 0) +
                  ((m & Qt::ControlModifier) ? 4 : 0);
        out = QByteArray("\x1b[") + QByteArray::number(n) + (mod == 1 ? QByteArray() : ";" + QByteArray::number(mod)) + "~";
    };
    switch (e->key()) {
    case Qt::Key_Return:
    case Qt::Key_Enter: out = "\r"; break;
    case Qt::Key_Backspace: out = (m & Qt::AltModifier) ? "\x1b\x7f" : "\x7f"; break;
    case Qt::Key_Tab: out = "\t"; break;
    case Qt::Key_Backtab: out = "\x1b[Z"; break;
    case Qt::Key_Escape: out = "\x1b"; break;
    case Qt::Key_Up: csi("\x1b[A", 'A'); break;
    case Qt::Key_Down: csi("\x1b[B", 'B'); break;
    case Qt::Key_Right: csi("\x1b[C", 'C'); break;
    case Qt::Key_Left: csi("\x1b[D", 'D'); break;
    case Qt::Key_Home: csi("\x1b[H", 'H'); break;
    case Qt::Key_End: csi("\x1b[F", 'F'); break;
    case Qt::Key_Insert: tilde(2); break;
    case Qt::Key_Delete: tilde(3); break;
    case Qt::Key_PageUp: tilde(5); break;
    case Qt::Key_PageDown: tilde(6); break;
    default: {
        const int k = e->key();
        if ((m & Qt::ControlModifier) && k >= Qt::Key_At && k <= Qt::Key_Underscore) {
            out = QByteArray(1, char(k - Qt::Key_At));
        } else if ((m & Qt::ControlModifier) && k == Qt::Key_Space) {
            out = QByteArray(1, '\0');
        } else {
            out = e->text().toUtf8();
        }
        if (!out.isEmpty() && (m & Qt::AltModifier))
            out.prepend('\x1b');
    }
    }
    if (out.isEmpty()) {
        e->ignore();
        return;
    }
    send(out);
    e->accept();
}

void GridItem::inputMethodEvent(QInputMethodEvent* e) {
    send(e->commitString().toUtf8());
    e->accept();
}

QVariant GridItem::inputMethodQuery(Qt::InputMethodQuery query) const {
    switch (query) {
    case Qt::ImEnabled: return true;
    case Qt::ImCursorRectangle:
        return QRectF(std::max(0, m_curX) * m_cellW, std::max(0, m_curY) * m_cellH, m_cellW, m_cellH);
    case Qt::ImFont: return m_font;
    default: return QQuickPaintedItem::inputMethodQuery(query);
    }
}

void GridItem::mousePressEvent(QMouseEvent* e) {
    forceActiveFocus();
    e->accept();
}

void grid_register() { qmlRegisterType<GridItem>("org.kyko.spike.native", 1, 0, "GridItem"); }

void grid_request_repaint() {
    // Called from the engine's thread.
    QMetaObject::invokeMethod(
        qGuiApp,
        [] {
            if (g_item)
                g_item->update();
        },
        Qt::QueuedConnection);
}
