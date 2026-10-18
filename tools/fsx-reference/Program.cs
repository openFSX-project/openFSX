// Independent local reference reader. Never linked into the openFSX engine.
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

internal static class Program
{
    private const uint DataMessage = 8;
    private static readonly string[] Names = {
        "elevator_pct", "elevator_deg", "trim_pct", "trim_deg",
        "left_aileron_pct", "right_aileron_pct", "rudder_pct", "rudder_deg",
        "yoke_x_pct", "yoke_y_pct", "throttle_pct", "on_ground"
    };
    private static readonly string[] Variables = {
        "ELEVATOR DEFLECTION PCT", "ELEVATOR DEFLECTION", "ELEVATOR TRIM PCT", "ELEVATOR TRIM POSITION",
        "AILERON LEFT DEFLECTION PCT", "AILERON RIGHT DEFLECTION PCT", "RUDDER DEFLECTION PCT", "RUDDER DEFLECTION",
        "YOKE X POSITION", "YOKE Y POSITION", "GENERAL ENG THROTTLE LEVER POSITION:1", "SIM ON GROUND"
    };
    private static readonly string[] Units = {
        "percent", "degrees", "percent", "degrees", "percent", "percent", "percent", "degrees",
        "percent", "percent", "percent", "bool"
    };
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true)]
    private static extern IntPtr GetProcAddress(IntPtr library, string name);
    [DllImport("kernel32.dll")]
    private static extern bool FreeLibrary(IntPtr library);
    [UnmanagedFunctionPointer(CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    private delegate int Open(out IntPtr connection, string name, IntPtr window, uint message, IntPtr ev, uint config);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int Close(IntPtr connection);
    [UnmanagedFunctionPointer(CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    private delegate int Define(IntPtr connection, uint id, string name, string units, uint type, float epsilon, uint datum);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int Request(IntPtr connection, uint request, uint definition, uint obj, uint period, uint flags, uint origin, uint interval, uint limit);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int Next(IntPtr connection, out IntPtr data, out uint size);

    private static T Function<T>(IntPtr library, string name) where T : class
    {
        IntPtr address = GetProcAddress(library, name);
        if (address == IntPtr.Zero) throw new InvalidDataException("Missing native export: " + name);
        return Marshal.GetDelegateForFunctionPointer(address, typeof(T)) as T;
    }
    private static void Check(int result, string operation)
    {
        if (result < 0) throw new IOException(operation + " failed: HRESULT 0x" + result.ToString("X8") + ". Start FSX and load a flight first.");
    }
    private static uint Word(byte[] packet, int offset) { return BitConverter.ToUInt32(packet, offset); }
    private static void Header(byte[] packet)
    {
        if (packet.Length < 12 || packet.Length > 1024 * 1024 || Word(packet, 0) != packet.Length)
            throw new InvalidDataException("Invalid SimConnect packet size.");
    }
    private static string Quote(string text)
    {
        StringBuilder result = new StringBuilder("\"");
        foreach (char c in text)
        {
            if (c == '"' || c == '\\') result.Append('\\').Append(c);
            else if (c < 32) result.Append("\\u").Append(((int)c).ToString("x4"));
            else result.Append(c);
        }
        return result.Append('"').ToString();
    }
    private static string Number(double value)
    {
        if (Double.IsNaN(value) || Double.IsInfinity(value)) throw new InvalidDataException("Non-finite reference value.");
        return value.ToString("R", CultureInfo.InvariantCulture);
    }
    private static string Sample(byte[] packet, double elapsed, string source, uint expectedObject = 0)
    {
        Header(packet);
        int expected = 40 + 256 + Names.Length * 8;
        if (packet.Length != expected || Word(packet, 8) != DataMessage || Word(packet, 12) != 1 || Word(packet, 16) != expectedObject ||
            Word(packet, 20) != 1 || Word(packet, 24) != 0 || Word(packet, 36) != Names.Length + 1)
            throw new InvalidDataException("Unexpected user-aircraft packet: size " + packet.Length +
                ", message " + (packet.Length >= 12 ? Word(packet, 8).ToString() : "truncated") +
                ", request " + (packet.Length >= 16 ? Word(packet, 12).ToString() : "truncated") +
                ", object " + (packet.Length >= 20 ? Word(packet, 16).ToString() : "truncated") +
                ", definition " + (packet.Length >= 24 ? Word(packet, 20).ToString() : "truncated") +
                ", flags " + (packet.Length >= 28 ? Word(packet, 24).ToString() : "truncated") +
                ", count " + (packet.Length >= 40 ? Word(packet, 36).ToString() : "truncated"));
        int end = Array.IndexOf<byte>(packet, 0, 40, 256);
        if (end < 0) throw new InvalidDataException("Unterminated aircraft title.");
        string title = Encoding.Default.GetString(packet, 40, end - 40);
        StringBuilder line = new StringBuilder("{\"schema\":1,\"source\":");
        line.Append(Quote(source)).Append(",\"elapsed_seconds\":");
        line.Append(Number(elapsed)).Append(",\"title\":").Append(Quote(title)).Append(",\"values\":{");
        for (int i = 0; i < Names.Length; i++)
        {
            if (i > 0) line.Append(',');
            line.Append(Quote(Names[i])).Append(':').Append(Number(BitConverter.ToDouble(packet, 296 + i * 8)));
        }
        return line.Append("}}").ToString();
    }
    private static int Capture(string folder, string output, int seconds)
    {
        if (IntPtr.Size != 4) throw new InvalidOperationException("Build this reference helper for x86.");
        string dll = Path.Combine(Path.GetFullPath(folder), "SimConnect.dll");
        if (!File.Exists(dll)) throw new FileNotFoundException("Expected SimConnect.dll inside the supplied FSX installation.");
        IntPtr library = LoadLibraryEx(dll, IntPtr.Zero, 8);
        if (library == IntPtr.Zero) throw new IOException("Cannot load the installed x86 SimConnect client; Win32 error " + Marshal.GetLastWin32Error());
        IntPtr connection = IntPtr.Zero;
        Close close = null;
        try
        {
            Open open = Function<Open>(library, "SimConnect_Open");
            close = Function<Close>(library, "SimConnect_Close");
            Define define = Function<Define>(library, "SimConnect_AddToDataDefinition");
            Request request = Function<Request>(library, "SimConnect_RequestDataOnSimObject");
            Next next = Function<Next>(library, "SimConnect_GetNextDispatch");
            // Config -1 forces a local connection and ignores network client settings.
            Check(open(out connection, "openFSX local reference reader", IntPtr.Zero, 0, IntPtr.Zero, UInt32.MaxValue), "SimConnect_Open");
            Check(define(connection, 1, "TITLE", null, 9, 0, UInt32.MaxValue), "Define aircraft title");
            for (int i = 0; i < Names.Length; i++) Check(define(connection, 1, Variables[i], Units[i], 4, 0, UInt32.MaxValue), "Define " + Variables[i]);
            Check(request(connection, 1, 1, 0, 2, 0, 0, 0, 0), "Request local user-aircraft visual frames");
            Stopwatch clock = Stopwatch.StartNew();
            int samples = 0;
            uint? userObject = null;
            using (FileStream file = new FileStream(output, FileMode.CreateNew, FileAccess.Write, FileShare.Read))
            using (StreamWriter writer = new StreamWriter(file, new UTF8Encoding(false)))
            {
                while (clock.Elapsed.TotalSeconds < seconds)
                {
                    IntPtr pointer;
                    uint size;
                    int result = next(connection, out pointer, out size);
                    if (result >= 0)
                    {
                        if (pointer == IntPtr.Zero || size < 12 || size > 1024 * 1024) throw new InvalidDataException("Invalid native dispatch buffer.");
                        byte[] packet = new byte[(int)size];
                        Marshal.Copy(pointer, packet, 0, packet.Length);
                        Header(packet);
                        uint id = Word(packet, 8);
                        if (id == 1)
                        {
                            if (packet.Length < 24) throw new InvalidDataException("Truncated SimConnect exception.");
                            throw new IOException("SimConnect exception " + Word(packet, 12) + ", send " + Word(packet, 16) + ", parameter " + Word(packet, 20));
                        }
                        if (id == 3) throw new IOException("FSX closed during reference capture.");
                        if (id == DataMessage)
                        {
                            if (packet.Length < 40) throw new InvalidDataException("Truncated user-aircraft data header.");
                            // Request object 0 is a user-aircraft alias. The response
                            // carries the real object ID; bind it only after the whole
                            // first packet has passed validation, then require stability.
                            uint actualObject = userObject ?? Word(packet, 16);
                            string line = Sample(packet, clock.Elapsed.TotalSeconds, "FSX SimConnect", actualObject);
                            if (!userObject.HasValue) { userObject = actualObject; Console.WriteLine("Bound local user-aircraft object " + actualObject + "."); }
                            writer.WriteLine(line); writer.Flush(); samples++;
                        }
                    }
                    else Thread.Sleep(5);
                }
            }
            if (samples == 0) throw new IOException("No aircraft samples received. Load an FSX flight; this run is not reference evidence.");
            Console.WriteLine("Recorded " + samples + " local reference samples. The tool sends no control events and changes no aircraft state.");
            return 0;
        }
        finally
        {
            if (connection != IntPtr.Zero && close != null) close(connection);
            FreeLibrary(library);
        }
    }
    private static byte[] Fixture()
    {
        byte[] data = new byte[40 + 256 + Names.Length * 8];
        Action<int, uint> set = (offset, value) => Buffer.BlockCopy(BitConverter.GetBytes(value), 0, data, offset, 4);
        set(0, (uint)data.Length); set(8, DataMessage); set(12, 1); set(20, 1); set(36, (uint)Names.Length + 1);
        byte[] title = Encoding.ASCII.GetBytes("Original synthetic \"aircraft\"\n");
        Buffer.BlockCopy(title, 0, data, 40, title.Length);
        for (int i = 0; i < Names.Length; i++) Buffer.BlockCopy(BitConverter.GetBytes(i == Names.Length - 1 ? 0.0 : i - 2.5), 0, data, 296 + i * 8, 8);
        return data;
    }
    private static void Reject(Action action)
    {
        try { action(); } catch (InvalidDataException) { return; }
        throw new Exception("Malformed synthetic packet was accepted.");
    }
    private static int SelfTest()
    {
        byte[] data = Fixture();
        string json = Sample(data, 1.25, "synthetic");
        if (!json.Contains("\\\"aircraft\\\"\\u000a") || !json.Contains("\"elevator_pct\":-2.5")) throw new Exception("JSON encoding or field offsets failed.");
        for (int n = 0; n < data.Length; n++) { byte[] truncated = new byte[n]; Buffer.BlockCopy(data, 0, truncated, 0, n); Reject(() => Sample(truncated, 0, "synthetic")); }
        byte[] invalid = Fixture(); invalid[24] = 1; Reject(() => Sample(invalid, 0, "synthetic"));
        invalid = Fixture(); Buffer.BlockCopy(BitConverter.GetBytes(Double.NaN), 0, invalid, 296, 8); Reject(() => Sample(invalid, 0, "synthetic"));
        invalid = Fixture(); for (int i = 40; i < 296; i++) invalid[i] = 65; Reject(() => Sample(invalid, 0, "synthetic"));
        byte[] realId = Fixture(); Buffer.BlockCopy(BitConverter.GetBytes((uint)1),0,realId,16,4);
        Sample(realId,0,"synthetic",1); Reject(() => Sample(realId,0,"synthetic",0));
        Buffer.BlockCopy(BitConverter.GetBytes((uint)2),0,realId,16,4); Reject(() => Sample(realId,0,"synthetic",1));
        Console.WriteLine("PASS: original synthetic dispatch layout, numeric fields, escaped JSON, all truncations, flags, non-finite values and missing terminator. No FSX DLL or server used.");
        return 0;
    }
    private static int Main(string[] args)
    {
        try
        {
            if (args.Length == 1 && args[0] == "--self-test") return SelfTest();
            if (args.Length == 1 && args[0] == "--synthetic-sample") { Console.WriteLine(Sample(Fixture(), 0.25, "synthetic")); return 0; }
            int seconds;
            if (args.Length != 6 || args[0] != "--fsx" || args[2] != "--output" || args[4] != "--seconds" || !Int32.TryParse(args[5], out seconds) || seconds < 1 || seconds > 300)
                throw new ArgumentException("Usage: fsx-reference --fsx <installation> --output <new.ndjson> --seconds <1..300> | --self-test");
            return Capture(args[1], args[3], seconds);
        }
        catch (Exception error) { Console.Error.WriteLine("Reference capture failed: " + error.Message); return 1; }
    }
}
