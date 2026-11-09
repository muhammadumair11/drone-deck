using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DroneCredentials {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct Credential {
  public uint Flags,Type; public string TargetName,Comment; public long Written;
  public uint Size; public IntPtr Blob; public uint Persist,Count; public IntPtr Attributes;
  public string Alias,User;
 }
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CredWriteW(ref Credential value,uint flags);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CredReadW(string target,uint type,uint flags,out IntPtr value);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CredDeleteW(string target,uint type,uint flags);
 [DllImport("advapi32.dll")] static extern void CredFree(IntPtr value);
 public static void Save(string target,string token) {
  byte[] bytes=Encoding.UTF8.GetBytes(token); IntPtr memory=Marshal.AllocHGlobal(bytes.Length);
  try {Marshal.Copy(bytes,0,memory,bytes.Length);var value=new Credential{Type=1,TargetName=target,User="HuggingFace",Size=(uint)bytes.Length,Blob=memory,Persist=2};
   if(!CredWriteW(ref value,0))throw new InvalidOperationException("Windows Credential Manager could not save the token.");
  } finally {for(int i=0;i<bytes.Length;i++){bytes[i]=0;Marshal.WriteByte(memory,i,0);}Marshal.FreeHGlobal(memory);}
 }
 public static string Read(string target) {
  IntPtr memory; if(!CredReadW(target,1,0,out memory)){if(Marshal.GetLastWin32Error()==1168)return null;throw new InvalidOperationException("Credential Manager is unavailable.");}
  byte[] bytes=null;
  try {var value=(Credential)Marshal.PtrToStructure(memory,typeof(Credential));bytes=new byte[value.Size];Marshal.Copy(value.Blob,bytes,0,bytes.Length);return Encoding.UTF8.GetString(bytes);}
  finally {if(bytes!=null)Array.Clear(bytes,0,bytes.Length);CredFree(memory);}
 }
 public static void Forget(string target){if(!CredDeleteW(target,1,0)&&Marshal.GetLastWin32Error()!=1168)throw new InvalidOperationException("Credential Manager could not remove this token.");}
}
