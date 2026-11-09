using System;
using System.IO;
using System.Net;
using System.Security.Cryptography;
using System.Text;
public static class DroneHub {
 static HttpWebResponse Open(string url,string token,long offset) {
  var uri=new Uri(url);
  for(int redirects=0;redirects<9;redirects++){
   string host=uri.Host.ToLowerInvariant();
   if(uri.Scheme!="https" || !(host=="huggingface.co"||host.EndsWith(".huggingface.co")||host.EndsWith(".hf.co")))throw new InvalidOperationException("Unexpected model download host.");
   var request=(HttpWebRequest)WebRequest.Create(uri);request.AllowAutoRedirect=false;request.Timeout=30000;request.ReadWriteTimeout=30000;request.UserAgent="DroneDeck/0.7";
   if(host=="huggingface.co"&&!String.IsNullOrEmpty(token))request.Headers["Authorization"]="Bearer "+token;
   if(offset>0)request.AddRange(offset);
   HttpWebResponse response;
   try{response=(HttpWebResponse)request.GetResponse();}catch(WebException error){var failed=error.Response as HttpWebResponse;int status=failed==null?0:(int)failed.StatusCode;if(failed!=null)failed.Dispose();throw new InvalidOperationException(status==401?"Hugging Face token is invalid or expired.":status==403?"Access denied. Accept the model license/gated access on Hugging Face, then retry.":status==404?"Repository, revision, or file not found.":status==429?"Hugging Face rate limit reached. Retry later.":"Model request failed (HTTP "+status+").");}
   int code=(int)response.StatusCode;
   if(code>=300&&code<400){var location=response.Headers["Location"];response.Dispose();uri=new Uri(uri,location);continue;}
   return response;
  }
  throw new InvalidOperationException("Too many model download redirects.");
 }
 public static string Json(string url,string token){using(var response=Open(url,token,0))using(var stream=response.GetResponseStream())using(var memory=new MemoryStream()){
  byte[] buffer=new byte[32768];int count;while((count=stream.Read(buffer,0,buffer.Length))>0){if(memory.Length+count>16000000)throw new InvalidOperationException("Repository metadata exceeds the supported limit.");memory.Write(buffer,0,count);}return Encoding.UTF8.GetString(memory.ToArray());}}
 static string Hash(string path,string sha256){using(var input=File.OpenRead(path))using(HashAlgorithm hash=String.IsNullOrEmpty(sha256)?(HashAlgorithm)SHA1.Create():SHA256.Create()){
  if(String.IsNullOrEmpty(sha256)){byte[] header=Encoding.UTF8.GetBytes("blob "+input.Length+"\0");hash.TransformBlock(header,0,header.Length,header,0);byte[] buffer=new byte[131072];int count;while((count=input.Read(buffer,0,buffer.Length))>0)hash.TransformBlock(buffer,0,count,buffer,0);hash.TransformFinalBlock(new byte[0],0,0);}else hash.ComputeHash(input);
  return BitConverter.ToString(hash.Hash).Replace("-","").ToLowerInvariant();}}
 static void Progress(string path,long done,long total,string phase){string temporary=path+".tmp";File.WriteAllText(temporary,"{\"bytes\":"+done+",\"total\":"+total+",\"phase\":\""+phase+"\"}");if(File.Exists(path))File.Replace(temporary,path,null);else File.Move(temporary,path);}
 public static bool Verify(string path,long size,string sha256,string blob){return File.Exists(path)&&new FileInfo(path).Length==size&&Hash(path,sha256)==(String.IsNullOrEmpty(sha256)?blob:sha256);}
 public static void Download(string url,string token,string target,long size,string sha256,string blob,string cancel,string progress,long completed,long total){
  if(size<0||(!String.IsNullOrEmpty(sha256)?sha256.Length!=64:blob==null||blob.Length!=40))throw new InvalidOperationException("This file has no usable published hash.");
  if(Verify(target,size,sha256,blob)){Progress(progress,completed+size,total,"Cached");return;}
  Directory.CreateDirectory(Path.GetDirectoryName(target));string partial=target+".partial";
  long offset=File.Exists(partial)?new FileInfo(partial).Length:0;
  if(offset>=size){using(var reset=File.Create(partial)){}offset=0;}
  using(var response=Open(url,token,offset)){
   if(offset>0 && (int)response.StatusCode!=206)offset=0;
   if(offset>0 && !(response.Headers["Content-Range"]??"").StartsWith("bytes "+offset+"-"))throw new InvalidOperationException("Server returned an inconsistent resume range.");
   using(var input=response.GetResponseStream())using(var output=new FileStream(partial,offset==0?FileMode.Create:FileMode.Append,FileAccess.Write,FileShare.Read)){
    byte[] buffer=new byte[131072];int count;long done=offset;DateTime last=DateTime.MinValue;
    while((count=input.Read(buffer,0,buffer.Length))>0){if(File.Exists(cancel))throw new OperationCanceledException("Download cancelled; verified-revision partial files retained for retry.");if(done+count>size)throw new InvalidOperationException("Download exceeds its reviewed size.");output.Write(buffer,0,count);done+=count;
     if((DateTime.UtcNow-last).TotalSeconds>=1){output.Flush();Progress(progress,completed+done,total,"Downloading");last=DateTime.UtcNow;}}
   }
  }
  Progress(progress,completed+size,total,"Verifying");if(!Verify(partial,size,sha256,blob))throw new InvalidOperationException("Model file hash/size verification failed. Retry restarts an invalid complete partial file.");
  if(File.Exists(target))File.Replace(partial,target,null);else File.Move(partial,target);Progress(progress,completed+size,total,"Verified");
 }
}
