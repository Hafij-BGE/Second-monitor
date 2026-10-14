package io.github.hafijbge.secondmonitor

import android.content.Context
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager

/**
 * Invisible view that receives the on-screen keyboard and forwards every
 * character and editing key to the PC.
 */
class KeyboardInputView(context: Context, private val input: InputController) : View(context) {
    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    /** Text the keyboard is still composing (already typed on the PC). */
    private var composing = ""

    override fun onCheckIsTextEditor() = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        // No suggestions: most keyboards then commit characters one by one.
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI or
            EditorInfo.IME_ACTION_NONE
        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
                replaceComposing(text.toString())
                composing = ""
                return true
            }

            override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
                replaceComposing(text.toString())
                return true
            }

            override fun finishComposingText(): Boolean {
                composing = ""
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                repeat(beforeLength.coerceAtMost(64)) { input.type(Keysyms.BACKSPACE) }
                repeat(afterLength.coerceAtMost(64)) { input.type(Keysyms.DELETE) }
                return true
            }

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                if (event.action != KeyEvent.ACTION_DOWN) return true
                val keysym = Keysyms.forKeyCode(event.keyCode)
                    ?: event.unicodeChar.takeIf { it != 0 }?.let { Keysyms.forChar(it) }
                if (keysym != null) input.type(keysym)
                return true
            }

            override fun performEditorAction(actionCode: Int): Boolean {
                input.type(Keysyms.RETURN)
                return true
            }
        }
    }

    /** Sends only the difference between the old and new composing text. */
    private fun replaceComposing(text: String) {
        var common = 0
        while (common < composing.length && common < text.length && composing[common] == text[common]) common++
        repeat(composing.length - common) { input.type(Keysyms.BACKSPACE) }
        text.substring(common).codePoints().forEach { cp ->
            if (cp.isPrintable() || cp == '\n'.code || cp == '\t'.code) input.type(Keysyms.forChar(cp))
        }
        composing = text
    }

    fun show() {
        requestFocus()
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
    }

    fun hide() {
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(windowToken, 0)
        composing = ""
        clearFocus()
    }
}
