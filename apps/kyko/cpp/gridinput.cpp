// The grid item's input: the mouse, the wheel, focus, keys and the input method.
#include "gridview_p.h"

#include <QtGui/QRegion>
#include <QtGui/QWindow>

#include <QtCore/QPointer>
#include <QtCore/QProcess>
#include <QtCore/QTimer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QClipboard>
#include <QtGui/QInputMethodEvent>
#include <QtGui/QKeyEvent>
#include <QtGui/QGlyphRun>
#include <QtGui/QPainter>
#include <QtGui/QPainterPath>
#include <QtGui/QTextLayout>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/QSGSimpleTextureNode>
#include <QtQuick/QSGTexture>
#include <QtQuick/QSGVertexColorMaterial>
#include <algorithm>
#include <cmath>
#include <cstring>

#include "kyko/src/bridge.cxx.h"

void GridView::sendMouse(quint8 kind, quint8 button, const QPointF& pos, Qt::KeyboardModifiers mods, quint8 clicks) {
    if (m_pane <= 0 || m_cols == 0 || m_rows == 0)
        return;
    const int col = std::clamp(int(pos.x() / m_cellW), 0, m_cols - 1);
    const int row = std::clamp(int(pos.y() / m_cellH), 0, m_rows - 1);
    const bool right = (pos.x() - col * m_cellW) * 2 >= m_cellW;
    quint8 m = 0;
    if (mods & Qt::ShiftModifier) m |= 1;
    if (mods & Qt::AltModifier) m |= 2;
    if (mods & Qt::ControlModifier) m |= 4;
    kyko_mouse(quint32(m_pane), kind, button, quint16(col), quint16(row), right, m, clicks);
}

static quint8 buttonCode(Qt::MouseButton b) {
    return b == Qt::LeftButton ? 0 : b == Qt::MiddleButton ? 1 : b == Qt::RightButton ? 2 : 3;
}

void GridView::scrollToY(qreal y) {
    const qreal f = std::clamp(y / std::max<qreal>(1, height()), 0.0, 1.0);
    kyko_scroll_to(quint32(m_pane), quint32(std::lround(qreal(m_scrollHist) * (1.0 - f))));
    update();
}

void GridView::mousePressEvent(QMouseEvent* e) {
    forceActiveFocus();
    if (e->button() == Qt::LeftButton && m_scrollOff > 0 && m_scrollHist > 0 && e->position().x() >= width() - 10) {
        m_scrollDrag = true;
        scrollToY(e->position().y());
        e->accept();
        return;
    }
    const QPoint cell(int(e->position().x() / m_cellW), int(e->position().y() / m_cellH));
    const qint64 now = e->timestamp();
    m_clicks = (now - m_lastPress < 400 && cell == m_lastCell && m_clicks < 3) ? m_clicks + 1 : 1;
    m_lastPress = now;
    m_lastCell = cell;
    m_dragged = false;
    m_down |= e->button();
    if (e->button() == Qt::MiddleButton && m_pane > 0) {
        const quint8 m = (e->modifiers() & Qt::ShiftModifier) ? 1 : 0;
        QClipboard* c = QGuiApplication::clipboard();
        if (c->supportsSelection() && kyko_middle_paste(quint32(m_pane), m)) {
            const std::string t = c->text(QClipboard::Selection).toStdString();
            if (!t.empty())
                kyko_paste(quint32(m_pane), rust::Str(t.data(), t.size()));
            e->accept();
            return;
        }
    }
    sendMouse(0, buttonCode(e->button()), e->position(), e->modifiers(), quint8(m_clicks));
    e->accept();
}

void GridView::mouseMoveEvent(QMouseEvent* e) {
    if (m_scrollDrag) {
        scrollToY(e->position().y());
        e->accept();
        return;
    }
    if (QPoint(int(e->position().x() / m_cellW), int(e->position().y() / m_cellH)) != m_lastCell)
        m_dragged = true;
    const Qt::MouseButton b = (m_down & Qt::LeftButton) ? Qt::LeftButton : (m_down & Qt::MiddleButton) ? Qt::MiddleButton : Qt::RightButton;
    sendMouse(1, buttonCode(b), e->position(), e->modifiers(), 0);
    e->accept();
}

void GridView::mouseReleaseEvent(QMouseEvent* e) {
    if (m_scrollDrag && e->button() == Qt::LeftButton) {
        m_scrollDrag = false;
        update();
        e->accept();
        return;
    }
    m_down &= ~Qt::MouseButtons(e->button());
    // A release reports the click count only for a click that did not drag, so the shell
    // cursor can follow a plain click.
    sendMouse(2, buttonCode(e->button()), e->position(), e->modifiers(), quint8(m_dragged ? 0 : m_clicks));
    if (e->button() == Qt::LeftButton && !m_dragged && m_clicks == 1 && m_pane > 0 && m_cols > 0) {
        const int col = std::clamp(int(e->position().x() / m_cellW), 0, m_cols - 1);
        const int row = std::clamp(int(e->position().y() / m_cellH), 0, m_rows - 1);
        quint8 m = 0;
        if (e->modifiers() & Qt::ShiftModifier) m |= 1;
        if (e->modifiers() & Qt::AltModifier) m |= 2;
        if (e->modifiers() & Qt::ControlModifier) m |= 4;
        kyko_open_link(quint32(m_pane), quint16(col), quint16(row), m);
    }
    e->accept();
}

void GridView::hoverLink(const QPointF& pos) {
    QList<quint16> spans;
    if (m_pane > 0 && m_cols > 0 && pos.x() >= 0 && pos.y() >= 0) {
        const int col = int(pos.x() / m_cellW), row = int(pos.y() / m_cellH);
        rust::Vec<quint16> out;
        if (col < m_cols && row < m_rows && kyko_link_spans(quint32(m_pane), quint16(col), quint16(row), out))
            for (quint16 v : out)
                spans.append(v);
    }
    if (spans == m_hover)
        return;
    m_hover = spans;
    setCursor(m_hover.isEmpty() ? Qt::IBeamCursor : Qt::PointingHandCursor);
    update();
}

void GridView::hoverMoveEvent(QHoverEvent* e) {
    hoverLink(e->position());
    // Only programs asking for all motion hear this; Rust drops it for the rest.
    if (m_down == Qt::NoButton)
        sendMouse(1, 3, e->position(), e->modifiers(), 0);
}

void GridView::wheelEvent(QWheelEvent* e) {
    // Whole notches: a touchpad's small steps add up until they make one.
    m_wheel += e->angleDelta().y();
    while (m_wheel >= 120) {
        sendMouse(3, 0, e->position(), e->modifiers(), 0);
        m_wheel -= 120;
    }
    while (m_wheel <= -120) {
        sendMouse(3, 1, e->position(), e->modifiers(), 0);
        m_wheel += 120;
    }
    e->accept();
}

void GridView::focusInEvent(QFocusEvent* e) {
    QQuickItem::focusInEvent(e);
    if (m_pane > 0)
        kyko_focus_report(quint32(m_pane), true);
    m_blinkOn = true;
    if (m_blink)
        m_blinkTimer.start();
    update();
}

void GridView::focusOutEvent(QFocusEvent* e) {
    QQuickItem::focusOutEvent(e);
    if (m_pane > 0)
        kyko_focus_report(quint32(m_pane), false);
    m_blinkTimer.stop();
    m_blinkOn = true;
    update();
}

void GridView::keyPressEvent(QKeyEvent* e) {
    // Typing shows the cursor at once and starts its blink again.
    if (m_blink) {
        m_blinkOn = true;
        m_blinkTimer.start();
    }
    quint8 mods = 0;
    if (e->modifiers() & Qt::ShiftModifier)
        mods |= 1;
    if (e->modifiers() & Qt::AltModifier)
        mods |= 2;
    if (e->modifiers() & Qt::ControlModifier)
        mods |= 4;
    if (e->modifiers() & Qt::MetaModifier)
        mods |= 8;
    const std::string text = e->text().toStdString();
    const quint8 event = e->isAutoRepeat() ? 2 : 1;
    if (kyko_key(quint32(m_pane), quint32(e->key()), mods, rust::Str(text.data(), text.size()), event))
        e->accept();
    else
        e->ignore();
}

void GridView::keyReleaseEvent(QKeyEvent* e) {
    if (e->isAutoRepeat()) {
        e->accept();
        return;
    }
    quint8 mods = 0;
    if (e->modifiers() & Qt::ShiftModifier)
        mods |= 1;
    if (e->modifiers() & Qt::AltModifier)
        mods |= 2;
    if (e->modifiers() & Qt::ControlModifier)
        mods |= 4;
    if (e->modifiers() & Qt::MetaModifier)
        mods |= 8;
    const std::string text = e->text().toStdString();
    if (kyko_key(quint32(m_pane), quint32(e->key()), mods, rust::Str(text.data(), text.size()), 3))
        e->accept();
    else
        e->ignore();
}

void GridView::inputMethodEvent(QInputMethodEvent* e) {
    const std::string text = e->commitString().toStdString();
    if (!text.empty())
        kyko_text(quint32(m_pane), rust::Str(text.data(), text.size()));
    e->accept();
}

QVariant GridView::inputMethodQuery(Qt::InputMethodQuery query) const {
    switch (query) {
    case Qt::ImEnabled: return true;
    case Qt::ImCursorRectangle:
        return QRectF(std::max(0, m_curX) * m_cellW, std::max(0, m_curY) * m_cellH, m_cellW, m_cellH);
    case Qt::ImFont: return m_font;
    default: return QQuickItem::inputMethodQuery(query);
    }
}
