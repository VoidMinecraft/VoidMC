import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassTransform;
import java.lang.classfile.CodeBuilder;
import java.lang.classfile.CodeElement;
import java.lang.classfile.CodeTransform;
import java.lang.classfile.Instruction;
import java.lang.classfile.instruction.ReturnInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;

/** Keeps the window hidden. `GLX._initGlfw` resets the hints, so they go at the end of `GlBackend.setWindowHints`. */
public class HideAgent {
  private static final ClassDesc GLFW = ClassDesc.of("org.lwjgl.glfw.GLFW");
  private static final MethodTypeDesc TWO_INTS = MethodTypeDesc.ofDescriptor("(II)V");
  private static final int COCOA_MENUBAR = 0x00051002;
  private static final int[][] HINTS = {{0x00020004, 0}, {0x00020001, 0}, {0x00023001, 0}};

  public static void premain(String args, Instrumentation instrumentation) {
    instrumentation.addTransformer(new ClassFileTransformer() {
      @Override
      public byte[] transform(ClassLoader loader, String name, Class<?> redefined, ProtectionDomain domain, byte[] bytes) {
        return switch (name) {
          case "com/mojang/blaze3d/platform/GLX" -> noMenuBar(bytes);
          case "com/mojang/blaze3d/opengl/GlBackend" -> hiddenWindow(bytes);
          case null, default -> null;
        };
      }
    });
  }

  private static byte[] hiddenWindow(byte[] bytes) {
    ClassFile file = ClassFile.of();
    return file.transformClass(file.parse(bytes), ClassTransform.transformingMethodBodies(
      method -> method.methodName().equalsString("setWindowHints"),
      (code, element) -> {
        if (element instanceof ReturnInstruction) {
          for (int[] hint : HINTS) {
            code.ldc(hint[0]);
            code.ldc(hint[1]);
            code.invokestatic(GLFW, "glfwWindowHint", TWO_INTS);
          }
        }
        code.with(element);
      }));
  }

  private static byte[] noMenuBar(byte[] bytes) {
    ClassFile file = ClassFile.of();
    return file.transformClass(file.parse(bytes), ClassTransform.transformingMethodBodies(
      method -> method.methodName().equalsString("_initGlfw"),
      CodeTransform.ofStateful(() -> new CodeTransform() {
        private boolean done;

        @Override
        public void accept(CodeBuilder code, CodeElement element) {
          if (!done && element instanceof Instruction) {
            done = true;
            code.ldc(COCOA_MENUBAR);
            code.ldc(0);
            code.invokestatic(GLFW, "glfwInitHint", TWO_INTS);
          }
          code.with(element);
        }
      })));
  }
}
